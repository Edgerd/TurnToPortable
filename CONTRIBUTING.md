# 贡献指南（CONTRIBUTING）

## 开发环境

- Rust stable（MSVC 工具链；发布以 windows-latest 原生构建为准）
- Flutter stable（仅改 `app/` 时需要）

## 本地验证（提交前全部跑一遍）

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace          # 单元 + e2e + property tests
cargo bench -p pxe-core         # 可选：检测路径基准
cd app && flutter analyze --fatal-infos && flutter pub get
```

CI（`.github/workflows/build.yml`）执行同样的门禁 + cargo-audit。

## 测试说明

- `crates/pxe-core/tests/e2e.rs`：打包管线端到端（不依赖二进制 fixture，全部用合成样本）
- `crates/pxe-core/tests/prop.rs`：proptest——任意/畸形输入喂给 detect 不得 panic
- `crates/pxe-core/src/detect.rs`、`extract.rs` 内嵌单测：格式识别与解包输出校验
- 新增格式支持时：至少覆盖 detect 单测；解包路径建议用脚本合成最小样本（如 pycdlib 造 ISO、wimlib-imagex 造 WIM）做端到端验证

## 代码约定

- 新格式支持一律**内嵌优先**（纯 Rust crate），外部工具仅作可选回退；回退链写在 `extract.rs` 的 `attempts` 表
- 文件名清洗、目录遍历等公共逻辑放 `fsx`，不要在 extract/builder 里重复实现
- CLI 与 GUI 共用 `pxe-core::pipeline`，不要在任一前端里私改流程逻辑
- 注释与文档用中文（与现有代码一致）

## 前端选择

`app/`（Flutter）为**主前端**，新功能与 UI 打磨优先落在这里；`pxe-gui`（egui）为轻量备用前端，保持与 pipeline 的同步即可。

## 发布流程

1. 更新 `Cargo.toml` 的 `workspace.package.version` 与 `CHANGELOG.md`
2. 打 tag：`git tag v0.x.y && git push origin main --tags`
3. CI 自动构建并发布 `PortableXE-v0.x.y-windows.zip` 到 GitHub Releases
