# Contributing to CC Switch Plus / 贡献指南

Contributions target this independent fork. For substantial changes, open an issue to discuss the behavior first. Small documentation fixes can go straight to a PR. Chinese and English are welcome.

本仓库的贡献面向独立维护的 Plus 分支。较大的变更请先通过 Issue 讨论；小型文档修复可直接提交 PR。欢迎中文和英文。

## Development / 开发环境

Use Node.js 22.12+, pnpm 10.12.3 (pinned in package.json), current stable Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/). The redesigned Skills workflow requires macOS.

```bash
git clone https://github.com/Andythropics/cc-switch-plus.git
cd cc-switch-plus
corepack enable
corepack prepare pnpm@10.12.3 --activate
pnpm install --frozen-lockfile
pnpm dev
```

The app shares the original CC Switch data directory. Quit other instances and back up your data before development. For manual backend experiments, `CC_SWITCH_TEST_HOME` selects an isolated application home; do not point it at your everyday home directory.

应用与原版共用数据目录。开发前请退出其他实例并备份数据。手动后端实验可用 `CC_SWITCH_TEST_HOME` 指定隔离目录，勿指向日常主目录。

## Validation / 验证

| Command                                                                     | Purpose / 用途                           |
| --------------------------------------------------------------------------- | ---------------------------------------- |
| `pnpm typecheck`                                                            | TypeScript                               |
| `pnpm format:check`                                                         | Frontend formatting / 前端格式           |
| `pnpm test:unit`                                                            | Frontend tests / 前端测试                |
| `pnpm build:renderer`                                                       | Production renderer build / 前端生产构建 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                    | Rust formatting                          |
| `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | Rust lint                                |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml`                  | Backend tests / 后端测试                 |
| `pnpm build`                                                                | Desktop package / 桌面打包               |

Run checks relevant to the change. Backend commands need a `dist/` directory; run the renderer build first in a fresh checkout. CI validates the frontend and macOS backend, the supported Plus Skills platform.

按修改范围运行相关检查。全新检出时先构建前端，确保后端需要的 `dist/` 存在。CI 检查前端与当前 Plus Skills 支持的 macOS 后端。

## Repository map / 代码导航

- `src/components/skills/`: Library, deployment, workspace, and migration interfaces.
- `src/hooks/useSkills.ts`, `src/lib/api/skills.ts`: frontend queries and typed API.
- `src-tauri/src/services/`: acquisition, validation, deployment, import, update, and migration.
- `src-tauri/src/database/`: persistence and schema migrations.
- `tests/`, `src-tauri/tests/`: frontend and backend tests.
- [CONTEXT.md](CONTEXT.md), [ADRs](docs/adr/): domain language and design decisions.

Read the domain docs before changing Skill ownership or deployment semantics. Add regression coverage for behavior changes; preserve unrelated user files and report stale observations.

修改 Skill 归属或部署语义前，请阅读领域文档。行为变更需要回归验证，注意保护用户的其他文件并报告过期观察结果。

## Pull requests / 提交 PR

1. Branch from `main`, keep the change focused, and describe the problem and resulting behavior.
2. Include validation evidence and UI screenshots when helpful.
3. Update user-facing text in all four files under `src/i18n/locales/`: `en.json`, `zh.json`, `zh-TW.json`, `ja.json`.
4. Update both READMEs when public behavior changes. Preserve upstream copyright notices.
5. You are responsible for understanding and testing AI-assisted contributions.

从 `main` 创建分支，保持修改聚焦，说明解决的问题与最终行为，附验证结果。UI 文案应同步四种语言；公共行为变化要同步中英 README。AI 辅助贡献同样需要作者理解并验证。

Use commit prefixes such as `feat(skills):`, `fix(deployments):`, `docs:`, and `ci:`. Contributions are provided under the project's [MIT license](LICENSE).

[Report a bug](https://github.com/Andythropics/cc-switch-plus/issues/new?template=bug_report.yml) · [Discuss an idea](https://github.com/Andythropics/cc-switch-plus/discussions) · [Security](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md)
