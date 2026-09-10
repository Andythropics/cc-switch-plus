# CC Switch Plus changelog / 增强版更新日志

The inherited upstream history remains in [CHANGELOG.md](CHANGELOG.md).

## 3.20.1-plus.2 — Skills workflow update / Skills 工作流更新

Based on CC Switch 3.20.1, including upstream work through `5a040348`.

- Adds external Skill source association and CLI installation synchronization.
- Presents available Skill updates in a review dialog.
- Unifies global and project Skill cards, clarifies deployment recovery actions, and highlights unavailable projects.
- Refactors application internals and fixes a Rust lint failure in deployment recovery.
- Replaces the README demo image with four app screenshots and recommends building locally from source in both languages.

新增外部 Skill 来源关联与 CLI 安装同步，集中审查可用更新；统一全局与项目技能卡片，明确部署修复操作并突出显示不可用项目。重构内部实现并修复 Rust 检查问题。中英文 README 更新为四张应用截图，并明确推荐本地源码构建使用。

**Preview:** macOS Apple Silicon and Intel; packages are not Apple Developer signed or notarized. Back up `~/.cc-switch/` and quit other CC Switch instances before upgrading.

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
