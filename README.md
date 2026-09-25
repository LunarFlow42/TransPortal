# TransPortal（轻量传送门）

开源桌面翻译与词典协同工作台。常驻系统托盘，通过全局快捷键捕获任意软件中选中的文本，自动判断"单词/句子"，分别以**深度词卡**或**幽灵伴读气泡**呈现。

## 技术栈

- **Desktop**: Tauri v2（`global-shortcut` / `clipboard-manager` / `single-instance` / `store` / `autostart` / `dialog` / `opener`）
- **Backend**: Rust 2024，`tokio` + `reqwest` + `reqwest-eventsource`（SSE），`redb`（嵌入式 KV），`windows-rs`（UI Automation + 系统 OCR），`ort` + `paddle-ocr-rs`（ONNX FP32 OCR）
- **Frontend**: React 19 + Vite + TypeScript + React Router（Hash）+ Tailwind CSS v4 + Framer Motion + Zustand + Lucide

## 核心能力

- **全局划词**：`Alt+T`（可配置）→ Windows 优先 UI Automation `TextPattern`，失败静默降级剪贴板；窗口在光标附近弹出并做屏幕边界钳制。
- **启动检查**：恢复已保存键位后检查注册冲突，浮层主动提示，并可在关闭占用软件后重新检测。Windows 发布版启动时验证整个 EXE 的 SHA-256，缺失或不匹配即提示退出；详见 [启动检查](docs/STARTUP_CHECKS.md)。
- **本地 OCR**：可切换 Windows 系统 OCR 与 ONNX · PP-OCRv4 FP32（中英混合识别），无需 Python 或 GPU。Windows 上 `Alt+Shift+T` 框选提取并复制文字，`Alt+Shift+S` 框选识别后翻译，框选器内置、不调用系统截图工具；浮层也可打开本地图片。ONNX 模型随桌面包分发，开发环境可一键安装，校验后离线使用。参见 [OCR 说明](docs/OCR.md)。
- **图片原位翻译**：保留截图与文字坐标，在原文字位置绘制译文；支持校对原文、切换目标语言、对照原图，以及复制或保存 PNG 译图。
- **自动区域翻译**：Windows 上框选固定屏幕区域，持续检查变化并更新译图；支持暂停、继续、停止与 `Esc` 取消。仅文字发送给翻译服务，截图不上传，重启不恢复监控。
- **双按 C 与文本替换**：Windows 上 `Ctrl+C+C` 取词翻译，普通复制不被抢占；结果卡可将原输入框选中的文字替换为译文，粘贴前核对窗口、焦点和选区。开关在快捷键设置中。
- **语义分流 + 原地形变**：单词→深度词卡，句子→幽灵气泡；Framer Motion `layoutId` 平滑形变。
- **查词来源全部可配置**：Youdao（在线，默认首选）、Google（在线）、本地 ECCEDICT（可选），支持启用/禁用/排序，带命中状态与延迟；在线结果有内存 LRU 缓存。
- **离线词库**：`redb` 存储 ECCEDICT，支持一键下载全量 `ecdict.csv`（带进度）或手动导入；`scripts/eccedict/build_enriched_csv.py` 可把 fork 的英美音标/标签 `.ddb` 合并回 CSV。
- **例句下钻**：交互式分词、点击生词原地切换、历史前进/后退。
- **Anki 同步**：Rust 侧 JSON-RPC 直连 AnkiConnect（8765），携带划词原句。
- **LLM 例句增强**：配置 OpenAI 兼容引擎后异步生成地道例句。

## 开发

前置：Node + pnpm、Rust（stable）、Windows 需要 WebView2（Win10/11 自带）。Linux 需要 Tauri 的系统依赖：`pkg-config libssl-dev libdbus-1-dev libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`（Debian/Ubuntu 包名）。

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发（Vite + Tauri）
pnpm build            # 仅构建前端
pnpm ocr:prepare      # 下载、校验并暂存当前平台 ONNX 模型与运行库
pnpm tauri build      # 构建桌面安装包（自动准备 ONNX 资源）
pnpm build:release    # 构建发布包及 SHA256SUMS（Windows 默认 NSIS，含自身校验文件）
# Linux：pnpm tauri build 生成 .deb / .rpm / .AppImage（AppImage 在无 FUSE 环境需 APPIMAGE_EXTRACT_AND_RUN=1）
pnpm test:integrity   # 校验文件生成工具测试
pnpm exec tsc --noEmit

cd src-tauri
cargo test            # Rust 单元测试
cargo clippy --all-targets -- -D warnings
bash ../scripts/check-architecture.sh   # core/platform 分层规则
```

## 目录

```
src/                  React 前端（views/store/hooks/components）
  views/ShellView     气泡/词卡叠加窗口 (#/shell)
  views/SettingsView  独立设置窗口 (#/settings)
src-tauri/src/
  lib.rs              组合根：注册服务、托盘、快捷键；commands.rs 为 IPC 适配层
  core/               跨平台业务逻辑（不含 cfg(target_os)，只依赖 platform 的接口）
    translation/      翻译引擎（Trait + 注册表：google_free / openai / tencent / youdao）
    dictionary/       查词编排（在线优先 + 可选本地 + LRU）、redb 词典与 ECCEDICT 导入、LLM 例句
    ocr/              引擎注册表、ONNX 推理、资源安装、截图与图片会话、自动区域翻译
    selection.rs      取词编排与原地替换令牌
    shortcut.rs       全局热键、按平台能力启用、窗口定位
    clipboard_monitor.rs  剪贴板翻译
    shell.rs intent.rs anki.rs integrity.rs
  platform/           操作系统接口（WindowOps / SelectionOps / ClipboardOps / GestureSource / DesktopCapture / SystemOps）
    mod.rs            Backend + Capabilities，唯一的平台切换点
    placement.rs imaging.rs key_sequence.rs   可移植纯逻辑（所有平台编译并测试）
    windows/          Win32 / UI Automation / DWM / DXGI / Windows OCR 实现
    linux/            Linux 后端（第一阶段委托 unsupported，文档列出后续计划）
    unsupported/      无原生实现时的回退
scripts/eccedict/     DuckDB → CSV 富化脚本
scripts/ocr/          资源打包与真实图片识别验证
docs/USER_GUIDE.md    用户操作文档
```

## 说明

- 在线词典默认走 Youdao；`dictionaryapi.dev` 未使用（其网络不稳定）。
- `ecdict.mini.csv` 仅约 53 词，**不是可用词库**；请使用全量 `ecdict.csv`。
- 设置（Provider/词典来源/快捷键/自启）自动持久化，重启恢复。
- Windows 发布版的 `integrity.json` 用于完整性对比，当前未签名；它不能代替病毒扫描或抵御程序与校验文件被同时替换。
- OCR 引擎与识别语言均持久化，切换失败保留原配置。内置框选器需要 Windows 10 2004+；`Esc` 可取消截图。macOS/Linux 当前通过本地图片使用 ONNX，桌面捕获适配器尚未实现；Linux 上划词读取剪贴板内容；双按 C、原地替换、截图 OCR 与自动区域翻译暂不可用：它们不出现在快捷键状态报告中（启动时不再报错），设置时提示「当前平台不支持此快捷键功能。」。
- ONNX 模型与运行库的版本、哈希和许可说明见 [OCR 资源清单](src-tauri/resources/ocr/manifest.json) 与 [第三方声明](src-tauri/resources/ocr/NOTICE.md)。体积、延迟和内存以实测为准，FP32 不保证与原框架逐字等同。

## License

MIT
