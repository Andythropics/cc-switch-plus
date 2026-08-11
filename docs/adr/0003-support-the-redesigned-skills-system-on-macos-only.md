# Support the Redesigned Skills System on macOS Only

The redesigned Skill Library, Global Workspace, Project Workspace, and unified Deployment model will be enabled only on macOS. Windows and Linux retain the rest of CC Switch but do not expose the redesigned Skills management UI, run its data migration, or preserve the legacy global-Skills implementation as a second behavioral model. Existing non-macOS Skill files and database state remain untouched.

The Skills navigation remains visible on unsupported platforms and opens a macOS-only explanation instead of the management UI, so users are not led to believe their existing data was deleted.

On macOS every Global or Project Skill Deployment uses a symbolic link, with no copy fallback. Each consumer's Skills root remains a real directory and CC Switch links individual Library Skills beneath it rather than linking the root itself. The first release supports Claude Code and Codex; Git-backed Project Workspaces are rooted at the repository or worktree root, and nested registrations inside the same repository are not supported.
