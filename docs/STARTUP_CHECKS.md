# 启动检查

## 快捷键冲突

应用先恢复保存的快捷键，再汇总注册结果。取词键（默认 `Alt+T`）、提取文字键 `Alt+Shift+T` 或截图翻译键 `Alt+Shift+S` 注册失败时，浮层会自动显示提示，列出具体键位与处理方式。关闭占用该键位的软件后，点击 **重新检测** 即可重试，无需重启 TransPortal。

设置的 **Global Shortcut** 分区持续显示各键位状态，也提供重新检测按钮。更改取词键注册失败时保留原有可用绑定；启动恢复失败时不会把临时回退键覆盖到保存的偏好中。重试成功后同步两个窗口、保存成功的取词键，并清除旧错误。

`Win+Shift+S` 由 Windows 截图工具处理，不抢占注册；只有用户把它绑定给 OCR 动作时才检查截图监听是否成功初始化。Windows 不提供通用接口来可靠列出某个全局键位由哪款软件占用，低级键盘钩子的拦截也不能通过注册检测穷尽，因此提示不会猜测占用者名称。

`Ctrl+C+C` 复用原生键盘观察器，在 500 ms 内两次独立的 Ctrl+C 按下时触发，所有输入原样传递。检测忽略本程序为取词、粘贴生成的按键，支持辅助功能或键位映射软件产生的输入。关闭开关会持久化并在启动检查前恢复；`Ctrl+C` 不允许注册为普通取词键，以保留系统复制功能。

## Windows 发布版自身校验

启动时读取程序旁的 `integrity.json`，核对版本、文件名、大小和整个 EXE 的 SHA-256。校验发生在 Tauri、托盘、快捷键、词典和 OCR 初始化之前。文件不匹配、校验文件缺失或格式错误时，显示原生错误提示并以退出码 `2` 结束，不自动生成或接受新的基准。

设置中的 **程序完整性** 分区可查看、复制实际 SHA-256，也可重新校验；重新校验失败会提示并退出。开发构建明确显示未执行发布校验，避免正常编译更新被误判。当前启动强制校验只针对 Windows；其他系统不会显示为校验通过。

校验文件目前**没有数字签名**。它能发现与该基准不同的程序文件，不能证明基准的发布者身份或程序没有病毒；如果程序和校验文件被一起替换，检查仍可被绕过。正式公开发行应再接入可信代码签名，并通过独立可信渠道公布发布校验值。

## 构建与发布文件

```powershell
pnpm build:release
```

Windows 默认生成 NSIS 安装包。Tauri 会先修改 EXE 的安装包类型标记；NSIS 的构建钩子随后运行 `scripts/integrity/seal.mjs`，对最终 EXE 生成校验值并把 `integrity.json` 装入安装目录。这里使用完整文件哈希，不排除任何可执行文件区段。当前没有为 MSI 或其他系统安装包配置对应的自身校验封装流程。

- `src-tauri/target/release/integrity.json`：`build:release` 为构建目录中可直接运行的程序生成的校验值。
- `src-tauri/target/release/transportal.exe.integrity.json`：安装包内程序对应的校验内容。
- `src-tauri/target/release/integrity-bundle/`：NSIS 读取的程序与校验文件快照，用于验证实际打包输入。
- `src-tauri/target/release/transportal.exe.sha256`：可执行文件的独立 SHA-256 校验行。
- `src-tauri/target/release/bundle/nsis/TransPortal_0.1.0_x64-setup.exe`：包含程序、OCR 资源及校验文件的安装包。
- `src-tauri/target/release/bundle/nsis/SHA256SUMS`：`build:release` 在 Tauri 完成打包后生成的安装包校验值。应随安装包一起提供，也可发布到独立可信位置供核对。

普通 `pnpm tauri build` 也会把自身校验文件装进 Windows NSIS 包；需要发布安装包校验文件时使用上面的 `pnpm build:release`。只执行 `cargo build --release` 得到的是尚未封装的中间程序，不应直接分发。重新编译或改变 EXE 后必须重新打包，不能沿用旧校验文件。

Tauri 在打包结束后会还原构建目录里 EXE 的包类型标记，因此它与安装包内的 EXE 字节不同。`build:release` 在最后为构建目录的 EXE 单独刷新 `integrity.json`，保留安装包和快照自己的校验值；两种文件都按完整字节计算，不混用基准。

## 验证

```powershell
pnpm exec tsc --noEmit
pnpm test
pnpm test:integrity
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
.\scripts\integrity\smoke-windows.ps1
```

完整性冒烟测试默认复制 `integrity-bundle/` 中的打包输入，在 `src-tauri/target/integrity-smoke/` 验证原文件通过、修改副本被拒绝、校验文件缺失/损坏被拒绝，并检查普通 GUI 启动确实弹出失败提示后退出。也可通过 `-Executable` 指定已封装程序。原发布文件不会被修改。报告保存在该目录的各次测试子目录中。

也可直接对最终程序运行 `transportal.exe --check-integrity`，读取 JSON 结果与退出码；该模式不初始化界面、快捷键或 OCR。

快捷键桌面验证使用临时进程占用 `Alt+T`、`Alt+Shift+S` 和一个自定义组合，检查启动提示、释放后的重试、保存偏好、回退绑定，以及未使用的默认键冲突不会误报。测试报告位于 `src-tauri/target/startup-e2e/`。
