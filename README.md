# PortableXE-RS

PortableXE 的 Rust 重写版：把 Windows 安装包转成免安装便携应用。

## 产物（dist/）

| 文件 | 平台 | 说明 |
|---|---|---|
| `pxe.exe` | Windows x64 | CLI：detect / extract / build |
| `pxe-gui.exe` | Windows x64 | GUI（egui + Fluent 2 主题） |
| `pxe-stub.exe` | Windows x64 | 原生便携启动器（替代 .bat，无黑窗） |
| `pxe-linux` / `pxe-stub-linux` | Linux x64 | 调试/测试用 |

## 用法

```powershell
# 检测
.\pxe.exe detect installer.exe

# 完整构建（检测→解包→生成便携包），打包为 zip
.\pxe.exe build installer.exe -n MyApp -o D:\PortableApps --pack zip

# 附带官方 VC++ 运行库
.\pxe.exe build installer.exe --redist vc_redist.x64.exe

# 开发环境把 stub 放同目录或设置：
$env:PXE_STUB_PATH = "pxe-stub.exe"
```

## 支持格式

输入：MSI / MSIX / AppX / Inno Setup / NSIS / WiX Burn / InstallShield / Squirrel / 7z / RAR / ZIP / TAR / 7z-SFX / 独立 EXE
输出：dir / zip / 7z

ZIP/7z/MSIX 解压内嵌于二进制，零外部依赖；innoextract / msiexec / unrar 为可选回退。

## 相比 Python 原版

- 真实签名检测（MSI 魔数、NSIS `NullsoftInst`、Inno 尾部 ID、`.wixburn` 节），无文件大小猜测
- 生成的 .bat 语法正确（`exit /b`），有测试锁定
- 不从 System32 盲拷 DLL，改为 `--redist` 附带官方安装器
- 外部工具调用带超时与 CREATE_NO_WINDOW，参数不经 `cmd /C` 转发
- 每个配置键均有消费者（测试锁定）；PE imports 真实解析
- 单文件分发，无需 Python 运行时

## 构建与测试

```bash
cargo test --workspace                 # 8 个测试
cargo build --release -p pxe-cli -p pxe-stub -p pxe-gui   # 本机
# Windows 交叉编译（zig + cargo-zigbuild）
cargo zigbuild --release --target x86_64-pc-windows-gnu -p pxe-cli -p pxe-stub -p pxe-gui
```

MIT License.
