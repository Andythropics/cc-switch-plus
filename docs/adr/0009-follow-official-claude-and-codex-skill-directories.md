# Follow Official Claude and Codex Skill Directories

The initial consumer Adapters will deploy Claude Code Skills to `~/.claude/skills/` globally and `<workspace>/.claude/skills/` per project, and Codex Skills to `~/.agents/skills/` globally and `<workspace>/.agents/skills/` per project. Migration removes only legacy entries under `~/.codex/skills/` that can be proven to have been created by CC Switch; real directories, foreign links, and other unmanaged content are preserved and reported rather than modified. Dual deployment to legacy and official Codex paths is intentionally avoided because it causes duplicate discovery.

The first release manages only these consumer roots directly beneath the registered Workspace root. Registration and reconciliation may report deeper `.claude/skills/` or `.agents/skills/` directories as unmanaged, unsupported scopes, but CC Switch neither deploys into them nor models nested Skill scopes in the MVP.
