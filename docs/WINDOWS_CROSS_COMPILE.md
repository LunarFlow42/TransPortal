# Linux 主机交叉编译 Windows (x86_64) 指南

本文档沉淀了在 Linux 环境下，将 TransPortal 编译并打包为标准 Windows NSIS 安装包（`TransPortal_<version>_x64-setup.exe`）的完整操作流程与排坑记录。

---

## 一键自动化构建

在配置好环境依赖后，仅需在项目根目录执行：

```bash
pnpm build:windows
```

该脚本会自动执行：
1. 工具链依赖自检（`makensis`, `clang`, `lld`, `cargo-xwin`）
2. 目标架构检查与安装（`x86_64-pc-windows-msvc`）
3. 下载与校验 Windows 版本的 ONNX OCR 运行时及离线模型
4. 静态编译前端资源（Vite -> `dist/`）
5. 运行 Tauri 生产打包管线，将前端资源 100% 离线注入内核二进制，生成 NSIS 安装包
6. 自动计算安装包 SHA-256 并生成 `.sha256` 校验文件

产物位置：
`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/TransPortal_<version>_x64-setup.exe`

---

## 环境准备与依赖安装

### 1. 系统底层工具
基于 Debian / Ubuntu 系统：
```bash
sudo apt update
sudo apt install -y nsis lld llvm clang
```

### 2. Rust Windows MSVC 目标
```bash
rustup target add x86_64-pc-windows-msvc
```

### 3. cargo-xwin 工具
`cargo-xwin` 负责在 Linux 上自动拉取和缓存微软官方 Windows SDK 与 MSVC CRT，免除手动配置 Windows 开发库的烦恼：
```bash
# 推荐使用 uv 或 cargo 安装：
uv tool install cargo-xwin
# 或者
cargo install --locked cargo-xwin
```

---

## 核心避坑指南 (Troubleshooting)

### 1. 为什么不能用裸 `cargo xwin build` 复制出 `.exe` 使用？
- **现象**：在 Windows 上运行直接从 target 拷出的 `transportal.exe`，窗口显示 `localhost refused to connect`（连接拒绝）。
- **原因**：Tauri 依赖构建时的上下文变量。没有走 `tauri build`，程序默认以 Dev 模式运行，WebView2 会尝试连接 `localhost:1420` 寻找 Vite 开发服务器。
- **正解**：必须通过 `pnpm tauri build` 进行全流程打包，并覆写 `{"build":{"devUrl":null,"beforeBuildCommand":null}}`，确保前端页面被以生产静态内嵌方式装入二进制。

### 2. 为什么安装包体积是 32MB 左右？
- 安装包使用 LZMA 高压缩比打包了完整的单文件 Tauri 内核、前端完整资源以及预置的 ONNX PP-OCRv5 离线多模型组件，用户无需预装 Python、CUDA 或配置额外网络即可离线正常使用。
