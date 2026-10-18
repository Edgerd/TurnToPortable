# 工作日志（会话级，推送后删除重建）

约定：本文件只记录当前未推送的改动。推送后删除本文件，下次改动时新建，便于统计每次推送的内容。

## 本次改动（2026-10-18）

1. 二进制产物出库：`git rm --cached` 6 个产物（`app/pxe.exe`、`dist/*`），`.gitignore` 增加 `/dist`、`app/*.exe`。文件仍在本地磁盘。
2. 新增 `LICENSE`（MIT，补齐 Cargo.toml 声明）。
3. `.github/workflows/build.yml`：加 `cargo fmt --check`、`clippy -D warnings`、`cargo test --workspace` 门禁；触发加 `pull_request` 与 `v*` tag；tag 时用 softprops/action-gh-release 发布 zip。
4. `app/lib/main.dart`：设置文件路径改用 `path_provider` 的 `getApplicationSupportDirectory()`（原硬编码 APPDATA/HOME，Linux 会写当前目录）；去掉裸 `catch (_) {}`，改为按异常类型 `debugPrint`。
5. `app/pubspec.yaml`：新增 `path_provider: ^2.1.0`（需 `flutter pub get` 后提交 pubspec.lock）。
6. clippy 清零：`pxe-gui` 移除未用 `Msg::Log`；`Config { ..Default }` 构造；`config::save` 去 `unwrap()`。

## 待办

- [x] Rust 验证：clippy 0 警告、测试全过、fmt 通过
- [ ] 验证：`app/` 内 `flutter pub get && flutter analyze`（沙箱无 Flutter，CI 会跑）
- [ ] 用户确认后提交并推送；推送后删除本文件
