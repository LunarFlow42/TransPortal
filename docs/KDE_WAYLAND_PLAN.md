# KDE Wayland 第二阶段

目标桌面是 KDE Plasma。保持 Windows 实现、三条静态窗口路由、设置持久化分工与词典/学习数据不变。仅处理用户发起的选区，不录屏、不采集周边文本、不使用 X11/XWayland 或读取 evdev。

## 实现与边界

- **按需 AT-SPI**：只有选区读取；替换额外比对唯一总线名称、对象路径、进程、范围、原文和可编辑状态。
- **浮层**：脚本使用 KWin 的光标与工作区逻辑坐标。可导入独立窗口规则，让明确标记的临时浮层不接收焦点。普通主窗口不使用标记。
- **被动手势**：可选版本匹配的 KWin 本地扩展，只接受用户登记的程序，只发送识别结果、进程和焦点代次。无需 X11、root 输入监听、InputCapture 或键位抢占。
- **一次性替换**：经用户显式启用后，对兼容的 text-input-v2/v3 应用提交译文。KWin 内部核对相同选区，不读写系统剪贴板、不使用整文覆盖或分步删除/插入。
- **RemoteDesktop**：keyboard-only Portal 授权、目标焦点票据、固定 Ctrl+V keysym 传输和失败释放均已实现并由私有 D-Bus 测试验证。KDE Portal 6.3 不授予 Clipboard Portal，因此该输入原语不接入翻译替换。按用户同意，兼容字段的替换主路径仍是 KWin text-input-v2/v3。

## 验证

- C++ 手势测试覆盖两次独立按键、释放控制键、单键落墨、长按、重复事件、取消及禁用。
- KWin 6.3.6 虚拟会话验证了真实 Qt 选区、Unicode 替换、旧票据拒绝、剪贴板保留，以及标记浮层定位和焦点保留。
- 另以 test-only 插件构建运行合成 Ctrl+C/Ctrl+C 与 Shift tap，端到端经过 KWin detector、定向 D-Bus、Rust 焦点校验和 AT-SPI 选区读取；含重复、修饰键、失焦、关闭和暂停的负向检查。该覆盖明确不是物理键盘测试。
- 私有总线 Rust 测试覆盖选区身份、Unicode、密码/只读、歧义焦点以及 RemoteDesktop 键盘授权取消。Windows 保持现有后端。
- 物理键盘、真实多屏及 Portal 授权界面仍需目标会话人工确认。

安装和验收步骤见 [KWin 扩展](../integrations/kwin/README.md)。持续翻译不在本阶段范围内。

## 最终自动验证记录

- Rust 全量：175 passed，4 ignored；其中 3 个需要隔离 KWin 会话的用例已另外运行，另 1 个是原有词典基准。
- 前端：29 文件、164 测试通过，TypeScript 和生产构建通过。
- Clippy `-D warnings`、rustfmt、架构规则、Windows MSVC 离线交叉检查通过。
- 生产 KWin 插件以 `-Wall -Wextra -Werror` 编译；二进制中不含 `TestKeyEvent` / `TestConfigure`。
- 生产插件下 Rust 实际选区替换在 text-input-v2 和 v3 均通过；test-only 插件下双复制/落墨的实际选区读取、负向事件过滤及浮层焦点保持通过。
