# CC Switch Plus changelog / 增强版更新日志

The inherited upstream history remains in [CHANGELOG.md](CHANGELOG.md).

## 3.20.1-plus.1 — Initial public preview / 首个公开预览版

Based on CC Switch 3.20.1, including upstream work through `5a040348`.

- Introduces the fork's private Skill Library and independent global / project deployments for Claude Code and Codex on macOS.
- Includes compatibility validation, unmanaged import, update review, deployment recovery, workspace lifecycle, and structured activity.
- Adds guided migration with backups, resume, and restore, while keeping device-specific deployment metadata local.
- Establishes the CC Switch Plus product identity, bilingual documentation, MIT attribution, contributor guidance, and macOS release workflow.
- Disables upstream automatic app updates; Plus releases are installed manually.
- Updates vulnerable frontend and macOS Rust dependencies and bundles LICENSE / NOTICE with the application. See [Security](SECURITY.md) for remaining Linux and build-tool dependency advisories.

基于 CC Switch 3.20.1，包含上游至 `5a040348` 的工作。首版公开技能库、Claude Code / Codex 全局和项目部署、兼容性检查、导入与更新审查、部署恢复、工作区生命周期及操作记录；提供带备份、续跑和恢复的迁移流程。建立独立品牌、双语文档、MIT 署名和 macOS 发布流程。

**Limits / 限制:** redesigned Skills are macOS-only; preview packages are not Apple Developer signed or notarized. Plus shares `~/.cc-switch/` with the original application. Read [installation and migration](docs/INSTALL.md) before upgrading.
