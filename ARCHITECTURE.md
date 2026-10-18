# 架构（ARCHITECTURE）

PortableXE 把「Windows 安装包」转换为「免安装便携应用」。所有流程逻辑收敛在 `pxe-core`，三个前端（CLI / egui GUI / Flutter GUI）只做输入与展示。

## 工作区

```
crates/
├── pxe-core    唯一流程实现：detect / extract / builder / pipeline / pe / fsx / config
├── pxe-cli     命令行前端（clap）：detect / extract / build（--json NDJSON 进度）
├── pxe-gui     egui 桌面前端（Fluent 2 暗色主题）—— 轻量备用前端
└── pxe-stub    便携包内原生启动器（环境重定向 + 退出码转发 + 启动日志）
app/            Flutter（Material 3）主前端，通过 pxe.exe --json 驱动
```

## 核心管线（pxe-core::pipeline::run）

```
安装包文件
   │
   ▼
detect ──────────── InstallerKind 判定
   │   · 流式读取：头部 4MB（魔数 / PE 标记 / 安装器特征串）+ 尾部 1KB（Inno 标记）
   │   · 恒为流式 SHA256（峰值内存 4MB，与文件大小无关）
   │   · 后缀 / 魔数 / 特征串三路证据，带回退链的 Kind 允许多候选
   ▼
extract ─────────── 解包（回退链）
   │   · 内嵌优先：zip / 7z / tar(+gz/xz/bz2/zst) / xz / bz2 / zst / iso / udf / merge-zip
   │   · 外部工具回退：innoextract / innounp / ISx / msiexec / lessmsi / unrar /
   │     expand·cabextract（IExpress）/ wimextract / external-7z
   │   · 同一 Kind 多候选按优先级依次尝试，输出经 validate()（须含 PE 或 manifest，无裸节碎片）
   ▼
deep_unpack ─────── 深度解包（可关）
   │   · 反复解 dest 内「最大的压缩档候选」，直到解出主程序（exe 须大于剩余最大压缩档）
   │   · 上限：8 层 × 每层 5 个候选
   ▼
locate main exe ─── 主程序判定（大小启发式，排除卸载器/碎片）
   ▼
builder::build ──── 便携包构建
   │   · 结构：App / Data(AppData·LocalAppData·Profile·Temp·Logs·Settings·Registry) /
   │     Documentation / Plugins / Redist + launcher.toml + portable.toml
   │   · 启动器：pxe-stub.exe（原生）+ .bat / .ps1 / RUN.bat 回退
   │   · 解压产物进入 App/ 用 rename 移动（跨设备回退拷贝）
   ▼
pack ────────────── Dir / zip / 7z 三种形态
```

## 关键契约

| 契约 | 位置 | 说明 |
|---|---|---|
| `launcher.toml` | 便携包根 | `exe` + `args`，pxe-stub / bat / ps1 共用 |
| `portable.toml` | 便携包根 | 元数据（应用名、来源安装器、via 解包器、构建时间） |
| NDJSON 进度 | `pxe build --json` stdout | 每行一个 `{"progress":0.15,"msg":"..."}`，Flutter 主前端解析 |
| 退出码转发 | pxe-stub | 等待主程序退出并以其退出码退出 |
| 启动日志 | `Data/Logs/launch.log` | UTC ISO8601 时间戳；>1MB 轮转为 `.old` |

## 解压回退链示例

`InstallerKind::InnoSetup` → `[innoextract → innounp → embedded-7z]`；
`Udf` → `[embedded-udf → embedded-iso]`（ISO/UDF 桥接镜像）；
`Wim` → `[wimextract → external-7z]`。

新增格式的原则：**内嵌优先**（纯 Rust crate），外部工具仅作可选回退；detect 提供「魔数 + 后缀」双证据；输出必须过 `validate()`。
