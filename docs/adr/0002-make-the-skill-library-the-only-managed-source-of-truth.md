# Make the Skill Library the Only Managed Source of Truth

Every Skill managed by CC Switch will exist exactly once in the private Skill Library at `~/.cc-switch/skills/`, and Project Workspaces will receive it through symbolic-link deployments rather than independent copies. Consumer discovery directories such as `~/.agents/skills/` are deployment targets, never Library locations. A project-local Unmanaged Skill must be explicitly imported into the Library, with backup and rollback protection, before its original location can be replaced by a deployment. Each deployment is recorded in local state and checked against the actual link; edits through any deployed path intentionally modify the Library Skill and affect every deployment. This sacrifices independently editable project copies in favor of stable centralized ownership, immediate propagation of Library updates, and freedom from copy divergence.

## Consequences

- Importing identical content reuses the existing Library Skill; differing content requires an explicit choice to import a new Skill or replace the Library version.
- A Library Skill has one unique directory name shared by every deployment; per-workspace aliases are not supported.
- A Library Skill's directory name is immutable after import. Its display name may change, but changing filesystem identity requires importing a new Skill and explicitly replacing deployments.
- Library Skills are direct children of `~/.cc-switch/skills/`; nested categorization is not part of filesystem identity. A directory-name collision requires the user to choose a readable unique name before import.
- Internal symbolic links are accepted only when relative, resolvable, and contained within the same Library Skill; absolute, broken, and escaping links are rejected.
- A Library Skill needs to pass structural validation for at least one supported Skill Consumer, not a lowest-common-denominator format for every consumer. The UI records Consumer Compatibility and disables incompatible deployment targets.
- Import preserves the Skill's bytes and unknown metadata. Validation uses each Consumer Adapter and never rewrites the Skill into a common schema; a missing consumer-specific requirement marks that Consumer incompatible rather than mutating the source.
- Workspace Unregistration preserves project contents and archives enough identity and deployment history to detect links that would otherwise become orphaned.
- Project deployments inside Git worktrees are added to the local `.git/info/exclude`; CC Switch does not modify a repository's committed `.gitignore`.
- If an Unmanaged Skill is already tracked by Git, CC Switch blocks automatic import-and-replace. The user must first make the repository-level change explicitly; local exclude rules cannot hide changes to tracked content.
- Migrating from `~/.agents/skills/` inventories the directory first, moves only known managed Skills into the private Library, recreates their intended deployments, and leaves Unmanaged Skills untouched until the user explicitly imports them.
- Global consumer roots are also inspected recurrently after migration. Real direct-child directories remain Unmanaged until the user explicitly imports a Library copy or confirms import-and-replace; inspection never adopts them from a matching name, and managed links are not import candidates.
- Removing a deployed Library Skill first removes every known reachable link. Any failed or unreachable deployment blocks Library deletion until the record is explicitly forgotten.
- Library updates validate a staged replacement and create a backup before atomically replacing the existing Skill directory, so every live deployment observes either the old complete version or the new complete version.
- A Library update is blocked if its staged replacement would become incompatible with any known active or archived Deployment. The affected deployments are listed, and the user must undeploy or explicitly forget them before applying the update.
- Local imports do not retain a live external source. Git and marketplace acquisitions may retain only portable Upstream Source metadata, and updating a locally modified Skill requires explicit confirmation and a backup.
- Destructive Library operations and storage migration retain the existing twenty most recent content backups; creating or removing a deployment does not copy or back up Skill content.
- Every deployment link points to the canonical absolute path of its Library Skill. Project links are local machine state and are excluded from version control, so portability of the link text is not a requirement.
