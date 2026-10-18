# 工作日志（会话级，推送后删除重建）

约定：本文件只记录当前未推送的改动。推送后删除本文件，下次改动时新建，便于统计每次推送的内容。

## 本次改动（2026-10-18）

1. 二进制产物出库：`git rm --cached` 6 个产物（`app/pxe.exe`、`dist/*`），`.gitignore` 增加 `/dist`、`app/*.exe`。文件仍在本地磁盘。
2. 新增 `LICENSE`（MIT，补齐 Cargo.toml 声明）。
3. `.github/workflows/build.yml`：加 `cargo fmt --check`、`clippy -D warnings`、`cargo test --workspace` 门禁；触发加 `pull_request` 与 `v*` tag；tag 时用 softprops/action-gh-release 发布 zip。
4. `app/lib/main.dart`：设置文件路径改用 `path_provider` 的 `getApplicationSupportDirectory()`（原硬编码 APPDATA/HOME，Linux 会写当前目录）；去掉裸 `catch (_) {}`，改为按异常类型 `debugPrint`。
5. `app/pubspec.yaml`：新增 `path_provider: ^2.1.0`（需 `flutter pub get` 后提交 pubspec.lock）。
6. clippy 清零：`pxe-gui` 移除未用 `Msg::Log`；`Config { ..Default }` 构造；`config::save` 去 `unwrap()`。
7. 消除全局静态配置：`extract::TIMEOUT_SECS/DEEP_EXTRACT` 静态开关删除，改为 `ExtractOptions { timeout, deep_extract }` 显式参数，贯穿 extract/deep_unpack/外部工具超时；pipeline 由 cfg 构造，CLI Extract 分支同样显式传入。
8. GUI↔CLI 协议升级：`pxe build --json` 输出 NDJSON 事件 `{"progress":0.15,"msg":"..."}`（serde_json）；Flutter 端加 `--json` 并解析 JSON 事件，移除旧 `[pxe|progress|...]` 文本协议。
9. 格式支持增强（外部工具全部为可选回退，内嵌优先）：
   - Inno Setup：回退链加 `innounp`（innoextract 1.9 解不动的 Inno 6.4+）；`flatten_app_dir` 兼容 innounp 的 `{app}` 输出目录
   - InstallShield/WiX Burn：回退链加 `ISx`（DevStudio 9 ~ IS 2021），以 cwd=dest 方式运行 + `flatten_single_dir` 展平输出
   - MSI：msiexec 失败后回退 `lessmsi`（免 msiexec、跨平台）
   - CI：随包 7-Zip 换为 myfreeer/7z-build-nsis v26.04 构建（NSIS 支持更广）
   - README 增加外部工具说明表
   - 未做：unshield（只认 .cab/.hdr 组合，不适用于 setup.exe 直接解包）；uninno（Perl 参考实现，仅作将来 Rust 移植蓝本）
10. 格式短板补齐（第二轮，全部内嵌优先、外部工具仅回退）：
   - 新增 Kind：`MsixBundle`/`IExpress`/`TarXz`/`TarBz2`/`TarZst`/`Xz`/`Bz2`/`Zst`/`Iso`/`Udf`/`Wim`/`Msu`/`Split7z`/`SplitRar`/`SplitZip`
   - MSIX/APPX bundle：detect 认 `.appxbundle`/`.msixbundle`，解压走 embedded-zip
   - IExpress（SFX-CAB）：detect 靠 PE 头 256KB 内 "IExpress" 标记；Windows 用 `expand -r`，非 Windows 回退 `cabextract`
   - ISO/UDF：纯内嵌（`iso9660` + `hadris-udf` crate），递归遍历 + `sanitize_disc_name` 清洗 ";1" 后缀与非法字符；UDF 失败回退 ISO 层（桥接镜像）
   - WIM/.msu：`wimextract`（wimlib）回退 external-7z；.msu 直接 external-7z
   - 分卷：`.7z.001`/`.part1.rar` 走首片解压；`.z01..zNN+.zip` 合并为临时 zip 再解（merge-zip）
   - xz/bz2/zst：`xz2`/`bzip2`/`zstd` 单文件流式解压 + 与 tar 组合（tar.xz/tar.bz2/tar.zst）
   - 端到端验证（沙箱实测）：ISO（pycdlib 造镜像）embedded-iso ✓；UDF（pycdlib 2.60）embedded-udf ✓；ZIP 分卷 merge-zip ✓；分卷 7z external-7z ✓；IExpress（cabarchive 造样本）cabextract ✓；WIM（wimlib-imagex 造样本）external-7z ✓；tar.xz/tar.bz2/tar.zst ✓
   - cargo test 12 过、clippy 0 警告
11. 外部评审遗留项一次性补齐（v0.3.0）：
   - pxe-stub：spawn 改为 wait + 退出码转发；append_log 改 UTC ISO8601（自带 civil_from_days 实现，零依赖）+ >1MB 轮转 launch.log.old；错误路径也 exit(1)
   - CI：加 rustsec/audit-check 安全审计；artifact/Release 文件名带版本（PortableXE-<ref>-windows.zip）
   - `.github/dependabot.yml`：cargo / pub / github-actions 三生态 weekly
   - `ARCHITECTURE.md`（管线图 + 契约表）、`CONTRIBUTING.md`（门禁/约定/发布流程）
   - README：双前端定位表（app/ Flutter 主前端，pxe-gui 备用）、已知限制、外部工具表补 cabextract/wimextract、构建说明改 MSVC 为主
   - property tests（proptest：detect 对畸形输入不 panic，256 cases）+ criterion 检测基准（`cargo bench -p pxe-core`）
   - **安全升级**：cargo-audit 查出 4 个漏洞全清——zip 2.4.2→9.0.5（RUSTSEC-2026-0367/0368/0369，符号链接逃逸/权限越界）；sevenz-rust 0.6（停维护+路径穿越 RUSTSEC-2026-0245）→ sevenz-rust2 0.24。适配 zip 9 API（name() 返回 Result<Cow>）与新 clippy（sort_by_key）
   - rustc 工具链默认切到 stable 1.96（sevenz-rust2 0.24 要求 ≥1.93）
   - 端到端复验：embedded-7z（sevenz-rust2 解）/ embedded-zip（zip 9 解）/ 7z 打包（compress_to_path 写→解全链路）均通过；cargo test 16 过、clippy 0、fmt 过、cargo audit 无漏洞（剩 5 个 egui 传递依赖 unmaintained 警告，不影响门禁）
   - 版本 0.2.0 → 0.3.0，CHANGELOG 补 v0.3.0 条目

## 待办

- [x] Rust 验证：clippy 0 警告、测试全过、fmt 通过、cargo audit 无漏洞
- [ ] 验证：`app/` 内 `flutter pub get && flutter analyze`（沙箱无 Flutter，CI 会跑）
- [x] 决策项：双 GUI 取舍——保留两者并在 README 声明主次（app/ 为主）
- [ ] 提交推送 + 打 tag v0.3.0 → CI 自动发 Release
