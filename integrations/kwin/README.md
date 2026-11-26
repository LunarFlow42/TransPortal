# KDE Plasma 6：浮层、手势与原地替换

仅适配原生 Wayland，不需要 root、X11、XWayland、后台光标采集或输入注入。
脚本在 KWin 内查询 `cursorPos` 和显示器工作区，以**逻辑坐标**移动明确标记的
TransPortal 浮层；不修改 `activeWindow`，不抓取 Ctrl+C 或任何其他按键。
应用仅在这次显示中关闭 GTK 的 focus-on-map，不调用主动激活；保留点击后输入
的能力。焦点最终仍由 KWin 决定，不能把 GTK 提示当成跨桌面的不激活保证。

## 鼠标旁浮层（脚本 + 窗口规则）

在仓库根目录的真实 Plasma 会话中执行：

```sh
kpackagetool6 --type KWin/Script --install integrations/kwin/transportal-overlay
```

在「系统设置 → 窗口管理 → KWin 脚本」启用 **TransPortal cursor overlay**。
在「系统设置 → 窗口管理 → 窗口规则」导入 [transportal-overlay.kwinrule](transportal-overlay.kwinrule)
并应用。此规则仅对标记为临时浮层的 TransPortal 窗口强制不接受焦点；通过托盘
或普通唤起打开主窗口时标题恢复正常，仍能编辑。只装脚本不装规则时，光标旁定位
仍有效，但 GTK 的提示不能强制 KWin 保留原焦点。

升级用 `--upgrade` 替代 `--install`。卸载：

```sh
kpackagetool6 --type KWin/Script --remove transportal-overlay
```

这是用户主动启用的合成器扩展，不会由应用自动安装/开启。未启用时浮层仍由
合成器正常放置，不会伪造全局光标位置或尝试用 GTK 绕过定位限制。

## 双复制、落墨与原地替换（可选原生插件）

原生插件是 KWin 进程内扩展，必须对照正在运行的 KWin 版本编译；KWin 升级后重建。
需要 CMake、Extra CMake Modules、C++20 编译器、Qt 6 开发包及与 KWin 匹配的 SDK
（Debian：`extra-cmake-modules`、`kwin-dev`）。
在仓库根目录构建：

```sh
cmake -S integrations/kwin/native -B build/kwin \
  -DTRANSPORTAL_QT_PLUGINDIR="$(qtpaths6 --query QT_INSTALL_PLUGINS)"
cmake --build build/kwin
ctest --test-dir build/kwin --output-on-failure
sudo cmake --install build/kwin
```

手动创建 `$XDG_CONFIG_HOME/TransPortal/KWinIntegration.conf`（默认 `~/.config/TransPortal/KWinIntegration.conf`）：

```ini
[General]
ApplicationExecutable=/absolute/canonical/path/to/transportal
EnableInPlaceReplacement=true
```

`ApplicationExecutable` 是正在运行的 **实际二进制**（可用 `readlink -f /proc/<PID>/exe` 查），
不是 `.desktop`、命令包装器或符号链接。只要手势就省略 `EnableInPlaceReplacement`。
KWin 用 D-Bus 的进程凭据验证监听者，不因任意进程知道接口名就交出手势或提交译文。
通过 `kwriteconfig6 --file kwinrc --group Plugins --key transportal-gesturesEnabled true`
显式启用，然后重新登录 Plasma Wayland。将该键设为 `false` 并重新登录可停用；清除上面的
配置即撤回应用登记。不要复制不可信的 `.so` 到 KWin 插件目录。

- **双复制 / 落墨**：在快捷键设置看到插件已连接后启用。KWin 不拦截或重放原按键，只发送
  已识别手势和焦点验证身份。需真实 libinput 键盘、原生 Wayland 源窗口与可访问选区；
  锁屏、换窗、其他按键、长按、合成键和选区失效不会触发，不读取旧剪贴板。
- **原地替换**：用桌面实际授权的 `Alt+Z`（默认建议值）。源字段需提供相符的 AT-SPI
  选区和启用中的 `text-input-v2/v3` 选区。KWin 只提交一次文本，不模拟粘贴、不改变剪贴板。
  截止时间 30 秒；中途编辑、换焦、点击、锁屏或选区变化后重选再试。密码、终端、不可编辑
  字段和协议不一致的客户端拒绝处理。GTK 3 会把部分字段的选区起止都报成光标位置，因此
  这些字段只能查词，不能原地替换。
- **RemoteDesktop 输入**：keyboard-only 授权后，独立的固定 Ctrl+V 原语经 Portal D-Bus
  发送键盘事件。KWin 每次确认目标 PID/焦点 epoch；拒绝把字节任意映射为按键，并在中途失败时
  释放按键。这个原语目前不接入翻译替换、不提供普通 UI 按钮，也不修改剪贴板。替换已按用户
  确认使用 text-input-v2/v3 主路径，不在不兼容时暗中回退到剪贴板粘贴。

## 验证

- 在支持 AT-SPI 的源程序中选中文字，按查词键，确认浮层靠近光标且源程序仍有焦点。
- 隐藏后再次触发，确认位置更新；移动光标不应让已经显示的浮层跟着跑。
- 多屏/混合缩放、负坐标、屏幕边缘要检查翻转和工作区裁剪。
- 手动打开主窗口、输入翻译、设置、OCR 窗口不能被该脚本挪动。
- 如果 GTK/应用 ID 不匹配，脚本保守地不做任何事；不要扩大成匹配所有窗口标题。

`bash integrations/kwin/native/smoke.sh build/kwin/transportal-gestures.so` 使用私有 D-Bus、
临时配置与 `kwin_wayland --virtual`，检查真实 Qt 编辑器替换及浮层定位、原焦点保留。
通过 `QT_WAYLAND_TEXT_INPUT_PROTOCOL=zwp_text_input_v2` 可验证另一套协议。提供
`RUST_SELECTION_PROBE`（`cargo test --lib --no-run` 输出的测试二进制的绝对路径）时，
还会使用 Rust 后端读取该真实 Qt 选区。也可用 `RUST_REPLACEMENT_PROBE` 指向同一个测试二进制，
验证 Rust 经 AT-SPI 和 KWin 票据实际提交并消耗替换请求（两种 probe 分开运行）。
设置 `RUST_GESTURE_PROBE` 可运行完整 Rust 往返：test-only KWin 注入入口生成合成 Ctrl+C/Ctrl+C
与 Shift tap，Rust 收 D-Bus 手势、校验焦点并实际读取 AT-SPI 选区。还验证重复键、带修饰键、
失效焦点、关闭双复制和暂停观察时拒绝触发。**这是合成事件，不是物理键盘验证。**该入口只在
`TRANSPORTAL_KWIN_SMOKE_TESTS=ON` 构建中编译，测试构建没有 install target；生产构建不含该入口。
物理按键、混合缩放和真实桌面 Portal 授权仍需手工检查。

手势链路测试使用独立构建目录，**不要安装测试插件**：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run
cmake -S integrations/kwin/native -B /tmp/opencode/transportal-kwin-test \
  -DTRANSPORTAL_KWIN_SMOKE_TESTS=ON
cmake --build /tmp/opencode/transportal-kwin-test
# 使用上面 Cargo 输出的测试可执行文件绝对路径（名字带构建 hash）：
RUST_GESTURE_PROBE=/absolute/path/to/transportal_lib-HASH \
  TMPDIR=/tmp/opencode bash integrations/kwin/native/smoke.sh \
    /tmp/opencode/transportal-kwin-test/transportal-gestures.so
```
