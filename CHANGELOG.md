# 更新日志

## v0.3.0 (2026-10-18)

### 新增

- **格式支持大扩展**（全部内嵌优先，外部工具仅可选回退）：
  - MSIX/APPX bundle（`.appxbundle`/`.msixbundle`，走 embedded-zip）
  - IExpress SFX-CAB（PE 头标记检测；Windows `expand` / 其他平台 `cabextract`）
  - ISO 9660 / UDF 光盘镜像（`iso9660` + `hadris-udf` 纯内嵌，";1" 后缀与非法字符自动清洗；UDF 失败回退 ISO 层）
  - WIM / .msu（`wimextract` 回退随包 7z）
  - 分卷压缩：`.7z.001` / `.partN.rar`（首片直解）、`.z01..zNN+.zip`（合并后解压）
  - xz / bz2 / zst 单文件流式解压及 tar.xz / tar.bz2 / tar.zst 组合（`xz2` / `bzip2` / `zstd`）
- **pxe-stub**：等待主程序退出并转发退出码；launch.log 改 UTC ISO8601 可读时间戳，超过 1MB 自动轮转 `.old`
- **工程化**：CI 增加 cargo-audit 安全审计；启用 Dependabot（cargo / pub / github-actions）；artifact 与 Release 文件名带版本号
- `ARCHITECTURE.md`（管线与契约）、`CONTRIBUTING.md`（贡献与发布流程）；README 增加双前端定位说明与已知限制
- property tests（proptest：畸形输入不得 panic）与 criterion 检测基准（`cargo bench -p pxe-core`）

### 修复

- IExpress 回退链标签（"expand" → 平台无关 "sfx-cab"，实际工具 Windows=expand / 其他=cabextract）
- **安全升级**（cargo-audit 发现 4 个漏洞全部清零）：
  - `zip` 2.4.2 → 9.0.5：修复符号链接逃逸、SUID/SGID 位保留、Unix 权限越界三个高危/严重漏洞（RUSTSEC-2026-0367/0368/0369）
  - `sevenz-rust` 0.6（停止维护 + 路径穿越 RUSTSEC-2026-0245）→ 维护中的 fork `sevenz-rust2` 0.24

### 测试与验证

- 单元 / e2e / property 测试全部通过；clippy 0 警告；fmt 通过
- 端到端实测：ISO（embedded-iso）、UDF（embedded-udf）、ZIP 分卷（merge-zip）、7z 分卷（external-7z）、IExpress（cabextract）、WIM（external-7z）、tar.xz / tar.bz2 / tar.zst
- CI 随包 7-Zip 换为 myfreeer/7z-build-nsis v26.04（NSIS 支持更广，LGPL + unRAR 许可合规再分发）

## v0.2.0 (2026-10-17)

### 新增

- **统一管线 `pxe-core::pipeline`**：CLI 与 GUI 共用同一套流程实现（识别 → 解压 → 便携包构建），保证两条入口行为一致
- **深度解包（可开关）**：解压后自动解内层最大的压缩档（按大小降序最多尝试 5 个），解出主程序即停止；CLI `--no-deep-extract` / GUI 复选框 / 配置 `deep_extract` 三处可关
- **应用图标**：pxe / pxe-gui / pxe-stub 三个 exe 嵌入纸箱图标（winresource）；GUI 窗口图标 + 界内 logo；Flutter 旧版资源同步替换
- `CHANGELOG.md`、新版 `README.md`

### 修复

- **检测误报**：安装器特征扫描窗口收紧到真实位置（NSIS 1MB / Inno 头 1MB+尾 1KB / SFX 1MB），并遍历所有 NSIS 签名命中位置——修复 pxe 自身被判为安装器、Qoder NSIS 被误判 7z-SFX 的问题
- pxe-stub 的 `App/Dependencies` PATH 前置从未生效（`PathBuf::push` 遇绝对路径整体替换 PATH），改用 `std::env::join_paths`
- GUI 字体加载把等宽字体插到 Proportional 首位，导致正文等宽渲染；Proportional / Monospace 现分开取首选字体
- 主程序判定：小卸载器（如 `$R0`）不再被误认为主程序（exe 须大于剩余最大压缩档才停）
- 7z 内嵌解压失败时自动回退外部 7z.exe
- 清理 GUI 冗余初始进度消息、未使用导入

### 优化

- SHA256 检测恒为流式计算：峰值内存从「文件大小（≤256MB）」降到 4MB
- 管线解压产物进入 `App/` 改为 rename 移动（同文件系统零拷贝，跨设备自动回退拷贝），GB 级应用省一整次全量 IO
- 目录遍历统一到 `fsx::walk_files(_skipping)`，替换 extract / launcher / 主程序判定三处重复实现；`find_7z` 与 `which` 合并

### 测试与验证

- 单元/端到端测试 10 个全部通过
- Qoder-CN 安装包（分卷 zip → NSIS → app-64.7z → Electron）端到端构建验证通过
- Windows x86_64-gnu release 交叉编译验证，dist/ 产物已刷新

## v0.1.x

- 初始版本：格式识别、NSIS/Inno/MSI/MSIX/压缩包解压、bat/ps1 启动器、egui Fluent 2 GUI、zip/7z 打包、原生 pxe-stub 启动器
