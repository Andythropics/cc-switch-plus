## CC Switch Plus — Initial public preview / 首个公开预览版

**One Skill library for Claude Code, Codex, and every project.**
**一个 Skill 库，连接 Claude Code、Codex 和你的每个项目。**

An independent MIT fork of CC Switch 3.20.1, including upstream work through `5a040348`. Original author credit and Git history are preserved.

基于 CC Switch 3.20.1 的独立 MIT 开源分支，包含上游至 `5a040348` 的工作，保留原作者署名与开发历史。

### Included / 本次包含

- Private Skill Library, per-consumer compatibility, and explicit acquisition versus deployment.
- Global and project deployments for Claude Code and Codex using shared Library links.
- Unmanaged Skill import, update review for local changes, deployment repair, and activity records.
- Guided migration with verified backups, resume, and restore.
- Plus branding, bilingual documentation, and its own release channel.

统一技能库、按工具兼容性检查、全局与项目部署、未托管导入、本地修改审查、部署修复和操作历史；提供带验证备份、续跑与恢复的迁移流程。

### Downloads / 下载

| Mac           | Files / 文件                      |
| ------------- | --------------------------------- |
| Apple Silicon | Filename contains `macOS-aarch64` |
| Intel         | Filename contains `macOS-x86_64`  |

Choose a DMG or ZIP. Both contain CC Switch Plus.app. Compare its SHA-256 with `SHA256SUMS`.

选择 DMG 或 ZIP，两者均包含应用。请与 `SHA256SUMS` 核对文件的 SHA-256。

### Before opening / 打开前

- **macOS 12+ preview.** Redesigned Skills currently support Claude Code and Codex on macOS only.
- **Shared data.** Disable the original CC Switch’s launch-at-login option, quit it, and back up `~/.cc-switch/` before first launch. Both apps share this directory; do not run them together. Older versions may require restoring the pre-upgrade backup.
- **No Apple Developer signing or notarization.** After verifying the download, use macOS Privacy & Security → Open Anyway if needed.
- **Manual application updates.** Upstream automatic updates are disabled.
- **Shared Skill edits.** Editing through a managed deployment changes the same Library Skill for all its deployments.

macOS 12+ 预览版，新版 Skills 仅支持 Claude Code 和 Codex。首次启动前请关闭原版开机自启、退出原版并完整备份数据目录。预览包尚未完成 Apple 开发者签名或公证；核验下载后可使用系统「仍要打开」。应用从本仓库手动更新，部署链接中的修改会影响共享技能库内容。

[English README](https://github.com/Andythropics/cc-switch-plus#readme) · [中文说明](https://github.com/Andythropics/cc-switch-plus/blob/main/README_ZH.md) · [Installation & migration / 安装迁移](https://github.com/Andythropics/cc-switch-plus/blob/main/docs/INSTALL.md) · [Feedback / 反馈](https://github.com/Andythropics/cc-switch-plus/issues)
