# Linux Wayland 原生适配

Linux 后端只面向 **Wayland**。不新增或维护 X11 实现，也不会通过 XWayland
截图、全局按键监听或输入注入绕过合成器的权限模型。Windows 后端保持独立。

## 已接入的能力

| 功能 | 实现与边界 |
| --- | --- |
| 全局快捷键 | `org.freedesktop.portal.GlobalShortcuts` v1+；桌面授权的实际键位优先于应用建议值 |
| 截图 OCR / 对照 / 原图翻译 / 搜索 | `org.freedesktop.portal.Screenshot` v2+；由桌面授权并选择截图，v3 支持时优先请求区域 |
| 划词 | 优先按需 AT-SPI 选区（只读选中文字）；不可用时 GTK Wayland primary selection，再降级到已复制文字 |
| 剪贴板文字、OCR 复制、译图复制 | GTK Wayland clipboard；所有 GTK 访问调度到主线程，读操作在工作线程等待，有超时及大小限制 |
| 窗口显示与焦点 | Tauri/GTK，使用 Portal 提供的短时 activation token；位置及焦点最终由合成器决定 |
| KDE 鼠标旁浮层 | 可选 KWin 脚本以合成器逻辑坐标定位；可导入随附窗口规则以保证不拿走原焦点 |
| KDE 双复制 / 落墨 | 可选原生 KWin 插件识别手势，不拦截现有按键，只读取焦点未变化的 AT-SPI 选区 |
| KDE 原地替换 | 可选原生 KWin 插件与显式启用；AT-SPI 和 text-input-v2/v3 双重验证，单次提交，不更改剪贴板 |
| 输入授权基础 | RemoteDesktop 的 keyboard-only Create/Select/Start、撤销、关闭；不请求录屏/鼠标，不发送按键 |
| 内存统计 | `/proc` 统计本进程与 WebKit 子进程 RSS、私有常驻页；权限不足时使用 RssAnon 近似值 |
| 内存修剪 | glibc `malloc_trim(0)` 尽力释放本进程的空闲分配器页，不清系统缓存、不强制回收 WebKit 子进程 |
| 查词、翻译、生词收藏、图片 OCR、TTS | 继续使用现有跨平台核心，不更换词库或用户数据 |

### 目前明确不支持

- ScreenCast/PipeWire 连续区域录屏与自动翻译：界面隐藏监控入口，后端也拒绝调用，
  不会用反复弹 Screenshot 授权来冒充持续捕获。
- 非 KWin 桌面的 Ctrl+C+C 与落墨手势，以及无 AT-SPI 或 text-input 支持的任意应用原地替换。
- RemoteDesktop 的 libei/EIS 或 D-Bus 通用输入注入。KDE 6.3 不提供该会话的剪贴板授权，
  所以 KDE 原地替换改用无需剪贴板的定向文本提交。
- 通用 Wayland 全局光标、强制窗口坐标。KDE 专用鼠标旁定位见
  [可选 KWin 脚本](../integrations/kwin/README.md)，未启用时仍由合成器放置。
- 保证所有应用提供 primary selection；这取决于源应用和合成器。
- 保证后台监听在所有桌面都能读取任意应用剪贴板。轮询只在用户开启后取文字，
  无法获得 offer 时失败，不降级到 XWayland，也不伪造剪贴板 revision。

## 环境准备

需要桌面会话 D-Bus、`xdg-desktop-portal` 及与你当前桌面匹配的后端。
安装 portal 总服务不代表该桌面已实现 Screenshot 或 GlobalShortcuts；后端能力
因版本和桌面而异。不要同时强制启用多个互相冲突的 portal 后端。

Debian/Ubuntu 开发依赖：

```sh
sudo apt install pkg-config libssl-dev libdbus-1-dev libgtk-3-dev \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  dbus-daemon xdg-desktop-portal
```

GNOME/KDE 通常由发行版安装对应 portal 后端；wlroots 桌面应核对 Screenshot 与
GlobalShortcuts 分别由哪个后端提供。不要仅因有 `DISPLAY` 就认为功能可用。

在**实际的 Wayland 桌面终端**中运行，不要从没有桌面会话环境的 SSH 终端验证 UI：

```sh
echo "$XDG_SESSION_TYPE"
echo "$WAYLAND_DISPLAY"
GDK_BACKEND=wayland pnpm tauri dev
```

## 使用流程

1. 首次启动/恢复设置后，程序合并快捷键设置为一次 Portal 会话请求，等待桌面授权。
   设置中的“等待桌面快捷键授权”不是注册成功，也不是启动错误。
2. 在桌面授权窗口确认快捷键；应用展示 Portal 返回的实际触发描述。
   拒绝、缺少后端或桌面撤销会话时展示错误，可在设置中重新检测/授权。
3. 在源应用选词后使用授权的查词键。应用先尝试 AT-SPI，在启用辅助功能的
   Qt/GTK 应用中不必先激活本程序。源应用不支持 AT-SPI 时才请求激活以接收
   primary selection；若仍不可用，先 `Ctrl+C` 再查词。不会自动模拟复制。
4. 点击截图功能，由合成器展示自己的截图/区域选择 UI。确认后图像只在本机进入
   ONNX OCR；文字是否发送到在线翻译服务由所选翻译源决定。
5. 取消应用中的捕获会关闭对应 Portal request；超时/授权失败不删除 Portal
   返回的文件，也不会让旧截图覆盖新 OCR 会话。

录制快捷键时只暂停应用对 Portal 激活的响应，合成器仍持有快捷键抓取。因此
更改键位可能需要在桌面授权窗口/系统快捷键设置中完成；不能承诺应用内能截获
合成器已抓走的按键。

## 测试与验证

```sh
cargo test --manifest-path src-tauri/Cargo.toml platform::linux
bash scripts/check-architecture.sh
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test
pnpm build
```

Portal 测试自行启动隔离的 `dbus-daemon`，不改进程全局环境，不访问开发者的
真实桌面授权服务，覆盖快速响应、取消/拒绝、会话关闭和实际键位反馈。

已在隔离的 KWin 6.3.6 虚拟 Wayland 会话运行 Qt 选区读取、原地文本提交、剪贴板保留、
浮层定位及焦点保留烟雾测试；**不等同于所有桌面或工具包都兼容**。
仍需在目标桌面手工验证：授权、重授权、撤销权限、复制后查词、截图取消、
多屏/混合缩放、托盘、设置窗口再次打开及 ONNX 推理。Linux 的 RSS/私有常驻页
也不能直接等同于 Windows Commit 指标。

KDE 安装、权限配置、协议限制以及虚拟会话测试见 [KWin 扩展](../integrations/kwin/README.md)。
持续翻译/ScreenCast/PipeWire 维持关闭。

### RemoteDesktop 授权输入原语

`authorize_wayland_input` 和 `revoke_wayland_input` 仅接受设置窗口调用，不在 bootstrap 中运行。
授权只申请键盘，不请求屏幕、鼠标或持久令牌。经授权会话可以传输固定 Ctrl+V，KWin
在每次发送前校验非自身目标 PID 和焦点 epoch，发送失败时尽力释放 Control/V。该能力
尚无普通 UI 操作入口，不用于文本替换，也不会借由覆盖剪贴板执行操作；keyboard-only
授权本身不会启用 `selection_replace`。

会话在用户撤销、重复授权、应用退出或 Portal `Session.Closed` 时关闭。
兼容文本字段的原地替换按用户确认采用 text-input-v2/v3，不以 RemoteDesktop 粘贴回退。
KWin 双复制/落墨往返烟测用 test-only 合成输入入口，不能替代真机物理键盘测试。
第二阶段设计与边界见 [KDE Wayland 计划](KDE_WAYLAND_PLAN.md)。
