# PortableXE-RS

<div align="center">
  <img src="assets/app_icon_256.png" width="128" alt="PortableXE">
</div>

**PortableXE 的 Rust 重写版** —— 把 Windows 安装包一键变成免安装便携应用。

拖进一个安装包（exe / msi / 7z / zip…），自动识别类型、解包（含嵌套安装器的深度解包）、定位主程序、生成带环境隔离的便携目录，可直接拷贝到 U 盘随身携带。

## 特性

- **智能识别**：NSIS / Inno Setup / InstallShield / MSIX / MSI / Squirrel / 7z-SFX / zip / 7z / tar.gz，流式检测 SHA256（4MB 峰值内存即可处理任意大文件）
- **深度解包**：electron-builder 一类的"套壳"安装器自动逐层解开（最多 8 层，每层最多尝试 5 个候选压缩档），**解出主程序即停**
- **原生启动器**：生成的便携包附带 `pxe-stub.exe` 原生启动器（无黑窗、带图标），APPDATA/TEMP 等环境变量重定向到 `Data/`，并回退生成 bat/ps1
- **标准便携结构**：`App / Data / Documentation / Plugins / Redist`
- **一套后端**：CLI 与 GUI 共用 `pxe-core::pipeline`，行为完全一致
- **多形态打包**：标准目录 / zip / 7z 单文件

## 产物（dist/）

| 文件 | 平台 | 说明 |
|---|---|---|
| `pxe.exe` | Windows x64 | CLI：detect / extract / build |
| `pxe-gui.exe` | Windows x64 | GUI（egui + Fluent 2 暗色主题） |
| `pxe-stub.exe` | Windows x64 | 原生便携启动器（无黑窗，放 pxe.exe 同目录即可） |
| `pxe-linux` / `pxe-stub-linux` | Linux x64 | 调试/测试用 |

## 用法

### CLI

```powershell
# 检测：类型 / PE 信息 / SHA256
.\pxe.exe detect installer.exe

# 提取（默认深度解包到主程序）
.\pxe.exe extract installer.exe -o .\extracted

# 完整构建便携包
.\pxe.exe build installer.exe -n MyApp -o D:\PortableApps

# 打包为 zip / 7z，附官方 VC++ 运行库
.\pxe.exe build installer.exe --pack zip --redist vc_redist.x64.exe

# 关闭深度解包（只解外壳）
.\pxe.exe build installer.exe --no-deep-extract
```

### GUI

运行 `pxe-gui.exe`：选择安装包 → 填应用名 → 选打包形态 → 构建，进度实时显示。

## 构建产物示例

```
Qoder_Portable/
├── Qoder.exe          # 原生启动器（写 Data/Logs/launch.log）
├── Qoder.bat / Qoder.ps1 / RUN.bat
├── launcher.toml
├── portable.toml
├── App/               # 应用本体（主程序在根或子目录）
│   └── Dependencies/  # （如有）自动前置到 PATH
├── Data/              # APPDATA / LOCALAPPDATA / Profile / Temp / Logs / Settings / Registry
├── Documentation/
├── Plugins/
└── Redist/            # 运行库说明 + 可选 vc_redist
```

## 从源码构建

```bash
cargo build --release --target x86_64-pc-windows-gnu   # Windows 产物
cargo test -p pxe-core                                 # 测试
```

工作区结构：

| crate | 职责 |
|---|---|
| `pxe-core` | 识别、解包（含深度解包）、启动器生成、打包 —— 唯一流程实现 |
| `pxe-cli` | 命令行前端（clap） |
| `pxe-gui` | 图形前端（eframe/egui，Fluent 2 主题） |
| `pxe-stub` | 生成的便携包所用原生启动器 |

## 更新日志

见 [CHANGELOG.md](CHANGELOG.md)。

## 许可

MIT
