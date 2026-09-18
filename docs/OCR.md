# 本地截图 OCR

支持 Windows 10/11 自带的 `Windows.Media.Ocr` 和 ONNX · PP-OCRv4 FP32 两种本地引擎，在设置的 **Screenshot OCR → 识别引擎** 中切换。识别在 Rust 工作线程中完成，无 Python 或 GPU 依赖。Windows 默认使用系统 OCR；ONNX 使用固定的中英混合模型，不需要系统语言包。

Windows 截图使用 TransPortal 自带的框选器：按下快捷键时立即冻结整个桌面并覆盖一层可拖动的选区窗口，松开鼠标后直接裁切冻结图像识别，不再调用系统截图工具，也不依赖剪贴板。macOS/Linux 当前可通过浮层的 **打开图片** 使用 ONNX，尚未实现桌面捕获适配器。

| 入口 | 行为 |
| --- | --- |
| `Alt+Shift+S` | 框选屏幕文字；识别文字、自动复制，并在浮层中编辑或复制 |
| `Alt+S` | 框选屏幕文字；保留图片与文字坐标，逐块翻译并在原位置绘制译文 |
| 浮层空态的「提取文字 / 截图翻译」按钮 | 对应两种截图流程 |
| 浮层空态的「打开图片」 | 读取本地 PNG / JPEG / BMP / WebP / TIFF，识别后编辑、复制或翻译 |
| 浮层空态的「自动区域翻译」 | 框选固定区域，持续检查屏幕变化并更新译图；可暂停、继续和停止 |
| 框选或识别时 `Esc` | 取消本次会话，丢弃迟到的结果 |

两个截图快捷键均用 Tauri 全局快捷键注册，可在设置中修改；其它程序占用时设置中显示错误，并可使用浮层按钮。更改取词快捷键只替换取词绑定，注册失败保留原绑定。仍可把 `Win+Shift+S` 绑定给某个 OCR 动作：这时通过原生键盘观察器识别后原样传递给系统截图工具，并等待其复制到剪贴板的图片（旧流程，需开启截图工具的自动复制）。

## 系统设置与文字处理

- 内置框选器需要 Windows 10 2004 或更新版本（用于把自身窗口排除在桌面捕获之外）。只有绑定 `Win+Shift+S` 走旧流程时，才需要系统截图工具开启自动复制到剪贴板。
- 选择 Windows OCR 时，设置列出已安装的识别语言。默认先按用户语言偏好创建识别器，若没有匹配偏好则使用第一个可用语言包；这不等于检测图片语言。
- 新装语言包后，点击 OCR 设置右侧的刷新按钮。中文截图建议明确选择中文识别器。
- 原生 OCR 的各行按返回顺序拼接并保留换行，不猜测句子边界或修改原文字。
- Windows OCR 对超过系统 `MaxImageDimension` 的图片按比例缩小。ONNX 检测时最长边限制为 1536 像素，并使用原图裁切识别；超过 3200 万像素或数据不完整的图片会被拒绝。小字可通过缩小截图选区或先放大文档改善。
- 无文本、语言包缺失、桌面捕获失败等错误均显示在浮层。旧流程下未收到图片的截图会话最多等待 60 秒。
- 纯 OCR 不启动在线词典、翻译或例句生成。点击「翻译 / 原位翻译」、使用 `Alt+Shift+S` 或显式启动自动区域翻译时才将文字交给当前 provider；截图图片不上传，仅在用户选择保存译图时写入文件。
- 纯 OCR 会将识别文字写入剪贴板。识别期间发生新的复制操作则不自动覆盖，并在结果中提示。截图翻译不改动剪贴板。

## 实现边界

```text
Alt+Shift+T / Alt+Shift+S / 浮层按钮 → RegionService 冻结桌面（DesktopCapture，排除自身窗口）
    → #/ocr-region 选区窗口 → 裁切冻结图像
Win+Shift+S（仅在用户显式绑定时） → ScreenCapture 观察系统截图 → 剪贴板新图片 + 截图工具来源校验
本地图片 → Rust 解码 + 尺寸限制
    → OcrService（会话、取消、串行推理）
    → OcrRegistry（当前引擎与识别语言的快照）
    → Windows OCR / ONNX FP32 → OcrEngine::recognize(RGBA, language)
    → transportal://ocr
        → text：可编辑文字 + 复制
        → translate：文字块 + 原图 → Rust provider 注册表 → Canvas 原位绘制 / PNG 导出
自动区域翻译 → 同一 RegionService 选区 → DesktopCapture 持续捕获（物理坐标，排除应用自身窗口）
    → 像素变化检查 → 同一 OcrService 推理锁 → 文字块翻译缓存 → transportal://auto-ocr
```

- `src-tauri/src/ocr/mod.rs`：与平台无关的 `OcrEngine`、`OcrImage`、`ScreenCapture` 接口和事件类型。
- `src-tauri/src/ocr/registry.rs`：引擎注册、配置验证和加载。成功加载后才更新配置，切回 Windows OCR 会释放 ONNX 模型会话；进程级运行库保留到退出。
- `src-tauri/src/ocr/onnx.rs`：CPU 推理、透明像素合成、深色背景预处理、方向识别，以及按行和横向位置排序文字块。
- `src-tauri/src/ocr/picture.rs`：原图坐标、PNG 内存编码、可取消的逐块翻译与会话缓存；单轮最多 3 个 provider 请求，单请求最多等待 30 秒。
- `src-tauri/src/ocr/monitor.rs`：持续区域监控；同一轮完成后才启动下一轮，相同像素跳过 OCR，已译文字块可复用，空白帧清除旧译文。停止或暂停会取消网络请求并丢弃迟到的识别结果。
- `src-tauri/src/ocr/region.rs`：截图与自动翻译共用的原生框选器。冻结整个虚拟桌面为一张图片，`#/ocr-region` 窗口按整个预览的相对坐标换算物理像素，支持负坐标屏幕；确认后按用途分发：截图会话直接裁切冻结图像识别，自动翻译则启动持续监控。图片解码完成后才显示窗口，避免黑屏闪烁。
- `src-tauri/src/native/desktop/`：Windows GDI 屏幕捕获与窗口排除。
- `src/utils/picture.ts`：按文字块周围颜色覆盖原字、选择对比色、缩放换行，在原图大小的 Canvas 上绘制。复杂纹理或小字区域的效果受检测框与背景采样影响，不做纹理重建；完整译文仍可在文字列表中读取。
- `src-tauri/src/ocr/assets.rs`：固定版本资源清单、SHA-256 校验、Rust 下载、白名单解压和安装进度。
- `src-tauri/src/ocr/workflow.rs`：一次只接受一个显式截图请求，空闲时不轮询图片或保存剪贴板历史。默认经内置框选器取图；仅当用户把 `Win+Shift+S` 绑定给 OCR 时才检查截图前后的剪贴板版本，并仅接收 Windows 截图工具进程发布的图片。
- `src-tauri/src/native/ocr/`：WinRT 初始化、OCR 语言、像素转换与系统截图键观察。平台条件只存在于原生工厂层。
- 原生识别无法强制中断时，取消会立即使会话失效；下一次识别等待上一轮结束，避免积累并发引擎实例。前端也按会话 ID 与阶段过滤重复、乱序事件。
- `SettingsView` 写入 `ocrEngine` 与 `ocrLanguage`，`ShellView` 只读取和接收设置同步。初始化完成前禁用 OCR 设置，避免启动恢复覆盖用户选择；选择失败保留原配置并显示错误。旧设置文件没有引擎字段时沿用本平台默认值。

自动模式的选区、图片、译文缓存仅存于内存，退出后不恢复。启动时快照当前 provider 和目标语言；修改后重新选区启动。自动模式使用 Windows 10 2004+ 的窗口捕获排除功能，启动失败时会显示原因。单个截图、包括选区预览，最多 3200 万像素。

## ONNX 模型与分发

当前固定使用 RapidOCR 发布的 PP-OCRv4 mobile 检测/识别模型与 `ch_ppocr_mobile_v2.0` 方向分类模型，通过 `paddle-ocr-rs` 0.6.1 + `ort` 2.0.0-rc.10 调用 ONNX Runtime 1.22.0 CPU。识别字典来自模型元数据。三份模型均为未量化 FP32；`onnx_inspect` 示例可以检查张量类型与量化算子。

- 支持准备 Windows / Linux / macOS 的 x86_64、aarch64 CPU 资源。当前实际运行验证在 Windows x86_64；其他平台的运行库已固定版本与哈希，仍需对应机器验证。
- `pnpm tauri build` 自动执行 `pnpm ocr:prepare`，把目标平台资源放入 `resources/ocr-bundle/`，再随应用分发。不包含下载缓存、其他平台运行库或调试符号。macOS universal 构建会包括两种架构。
- 开发时运行 `pnpm ocr:prepare`，或在设置中点击 **安装并启用 ONNX**。所有下载发生在 Rust，前端仅接收进度。下载地址、大小和 SHA-256 固定在 [manifest.json](../src-tauri/resources/ocr/manifest.json)。
- Windows x86_64 三份模型约 15.4 MiB，模型与运行库核心资源共约 27.6 MiB。首次按需下载约 84.5 MiB，因为微软 ZIP 还包含不参与打包的调试符号等文件；已校验缓存可复用。
- 打包资源优先使用；资源缺失或损坏时，设置中的安装入口写入应用数据目录的 `ocr/`，不修改安装目录。未通过校验的文件不能加载，部分下载可安全重试。
- 第三方模型和库的许可文本、来源与声明随资源分发，见 [NOTICE.md](../src-tauri/resources/ocr/NOTICE.md)。后续引擎实现 `OcrEngine` 并注册后即可复用同一截图、图片、事件和翻译流程。

FP32 不自动保证逐字符等同原框架，预处理、字典、导出算子和后处理都会影响结果。此前设想的 35 ms、45 MB 或「100% 无损」不是保证值。基准须区分安装包与展开体积、进程总内存与 OCR 增量内存、冷启动与热运行、以及包括框选和渲染的端到端时间。当前未提供 FP16/INT8 选项。

## 验证与诊断

常规检查：

```powershell
pnpm exec tsc --noEmit
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

运行真实 ONNX FP32 识别与发布版基准：

```powershell
pnpm ocr:prepare
.\scripts\ocr\smoke-onnx.ps1 -Release
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr_probe -- --engine onnx --image src-tauri/tests/fixtures/ocr/mixed-light.png --repeat 3
```

固定图片集覆盖英文浅色、中英混排、英文深色、透明图、180° 旋转、空白图和 1 像素图。测试断言实际文字（忽略独立文字块之间的空白差异）和行数，结果与冷/热时间写入 `src-tauri/target/ocr-smoke/onnx-release.json`。空白和极小图片在引擎层返回空文字，在应用层显示「未识别到文字」。样例通过不等于通用准确率评估。

### 图片翻译、双按 C 与文本替换（2026-09-15）

- `cargo check`、32 项 Rust 测试、33 项前端测试和生产前端构建通过。
- Windows OCR 与 ONNX 均已在桌面程序中验证文字坐标、原位绘制、校对后重新翻译和 PNG 导出；检查了亮暗主题。
- 原生 `Ctrl+C+C`、单次复制及按住 C 连发不误触发、开关生效、输入框替换、撤销、过期选区拒绝替换均已验证。替换时会保存实际剪贴板数据；文字和自定义格式的恢复已验证。
- **自动区域翻译仍待手动验收**：框选窗口、持续更新、暂停/继续、停止和 `Esc`。代码已实现，尚未完成这些操作的桌面端到端验收，不能视为全部验证通过。操作入口见 [使用指南](USER_GUIDE.md#34-持续自动翻译屏幕区域)。

### 此前 ONNX 接入验证记录（2026-09-15，较早版本）

- `cargo check`、21 项 Rust 测试、TypeScript 检查、24 项前端测试和生产构建通过。Clippy 的剩余提示位于既有词典与 OpenAI provider 代码。
- 7 组发布版真实图片识别通过。桌面 Tauri 窗口实测通过：设置切换到 ONNX、纯 OCR 不发起翻译、编辑原文后经 Rust provider 翻译、切回 Windows OCR、再次启用 ONNX，以及退出后重启恢复。亮暗主题均检查了实际渲染。
- 自动复制成功和剪贴板占用时的错误提示分别验证。占用测试使用隐藏窗口持有剪贴板，并断言剪贴板版本不变；识别结果与编辑入口始终保留。
- NSIS 安装包约 20.5 MB。应用程序与 OCR 资源展开约 35.4 MB，不包含系统 WebView2、本地词库及用户数据。安装脚本的资源目标目录、全部模型/运行库哈希及许可文件已核对；安装向导本身未执行。
- 使用打包目录中的资源，对 900 × 180 的中英混排图片连续识别 20 次：首次 **805 ms**，后 19 次中位数 **272 ms**，范围 **253–645 ms**。无界面探针的进程峰值工作集 **79.4 MiB**，采样私有内存峰值 **67.1 MiB**；这些不是完整 Tauri 应用内存，也不是 OCR 相对空进程的增量。

当前样例未达到 `<50 ms` / `<50 MB` 的原先性能设想；首轮包含资源校验与模型加载，后续轮次复用会话，均不包括框选、剪贴板和翻译请求。原始报告在 `src-tauri/target/ocr-smoke/`，桌面验证报告在 `src-tauri/target/ocr-e2e/`。

早期独立检查旧的 `Alt+S` 注册返回过 Windows 错误 1409（已被其他程序占用），因此默认截图键改为 `Alt+Shift+S`；当前是否冲突以启动提示和快捷键状态面板为准。内置框选交互、多屏 DPI、安装向导及其他操作系统仍需对应环境的手动验收。

列出系统 OCR 语言包（不打开窗口、不访问屏幕或剪贴板）：

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --example ocr_probe
```

在 Windows 上用人工生成的两行英文实际识别并断言结果：

```powershell
.\scripts\ocr\smoke-windows.ps1
# 可通过 -Language 指定已安装的识别语言；图片输出到被 git 忽略的 target/ocr-smoke。
```

手动检查系统交互：

1. 重启 `pnpm tauri dev`。在屏幕或 PDF 上分别用两组快捷键框选，确认框选层立即出现（不经过系统截图工具）、纯 OCR 与翻译行为不同，中文和英文语言包都可选择。
2. 框选前先把另一张图片放入剪贴板，然后按 `Esc` 取消。确认旧图片不会被识别；再次截图可正常完成。
3. 框选时按 `Esc`，快速重复快捷键，识别时取消后重新触发，确认没有重复翻译或旧文字覆盖。
4. 更改取词快捷键后确认 `Alt+Shift+S` 仍能使用；尝试把取词键设置为 `Alt+Shift+S` 或 `Win+Shift+S` 应提示保留旧取词绑定。
5. 在亮/暗主题和多屏不同 DPI 下，检查识别文字编辑、复制、翻译、点词返回、浮层尺寸及设置的重启恢复。
6. 在设置中切换 ONNX，用「打开图片」识别中英文图片；修正文字后点击翻译。切回 Windows OCR 再次识别，重启后确认所选引擎恢复。缺失资源或加载失败应显示错误并保留原配置。

控制台 `[ocr]` 日志只记录会话、引擎、语言、字符数和识别耗时，不记录识别原文或图片。结果中的毫秒数仅包含该次 OCR 调用及预处理，不包含用户框选、剪贴板等待和译文请求。
