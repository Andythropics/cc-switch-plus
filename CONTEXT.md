# CC Switch

CC Switch manages reusable AI tool configuration and capabilities across user-wide and project-specific scopes.

## Language

**Skill Library**:
The sole authoritative collection of every Skill managed by CC Switch on a device. A managed Skill has no independently owned workspace copy.
_Avoid_: Skill workspace, project Skills

**Library Skill**:
A managed Skill stored as a direct child of the Skill Library, with one stable identity and one immutable unique directory name used unchanged by every Skill Deployment.
_Avoid_: Project variant, deployment alias

**Upstream Source**:
The remote Git or marketplace origin from which a Library Skill may check for updates. A local import has no continuing external source after the Library assumes ownership.
_Avoid_: Local source directory, live source

**Local Modification**:
A change to a Library Skill since its recorded upstream version, including a change made through any live Skill Deployment.
_Avoid_: Project override, deployment change

**Project Workspace**:
A registered working project, uniquely identified by its physical directory, whose local Skill set applies only within that project. Its display name defaults to the directory name but may change independently; a version-controlled project uses its repository or worktree root and may have only one registered workspace.
_Avoid_: Project Profile, Project Snapshot

**Active Workspace**:
A Project Workspace currently available for inspection and Skill Deployment.
_Avoid_: Current project, selected workspace

**Archived Workspace**:
A Project Workspace hidden from active management while its identity and deployment history are retained. It cannot receive or repair deployments until restored.
_Avoid_: Deleted workspace, inactive project

**Unavailable Workspace**:
An Active Workspace whose physical directory cannot currently be reached. It may be relocated, archived, or forgotten but cannot be mutated while unavailable.
_Avoid_: Missing project, deleted workspace

**Global Workspace**:
The user-wide Skill scope of one Skill Consumer, independent of any Project Workspace.
_Avoid_: Global Project, default project

**Skill Consumer**:
An AI tool that discovers and loads deployed Skills, such as Claude Code or Codex.
_Avoid_: App, Agent

**Consumer Compatibility**:
The set of supported Skill Consumers for which a Library Skill passes structural validation. A Library Skill may be compatible with only one consumer, and incompatibility prevents deployment to that consumer without preventing Library ownership.
_Avoid_: Universal Skill, shared format

**Skill Validation**:
The read-only, per-consumer assessment of a Skill's canonical `SKILL.md`, metadata, supporting files, and contained links. Validation records compatibility without normalizing or rewriting the Skill.
_Avoid_: Format conversion, metadata normalization

**Configuration Profile**:
A named reusable snapshot of AI tool configuration that is independent of every workspace and never contains Skill Deployments.
_Avoid_: Project, Workspace

**Skill Deployment**:
The live exposure of a Skill from the Skill Library to one Skill Consumer in a Global or Project Workspace. Deployments are assigned independently per consumer, never create another authoritative copy, and changes made through any deployed location affect the same Library Skill.
_Avoid_: Installation, global enablement

**Deployment Drift**:
A mismatch between a recorded Skill Deployment and the link observed at its target location. Drift remains visible until a user explicitly repairs or forgets it.
_Avoid_: Automatic repair, automatic adoption

**Deployment Conflict**:
Deployment Drift in which the target path is occupied by a real directory or a link to another source. A conflict is never overwritten automatically.
_Avoid_: Name collision, automatic replacement

**Skill Import**:
The explicit admission of an unmanaged Skill into the Skill Library, after which the Library owns the managed version.
_Avoid_: Project-to-Library sync, adoption

**Unmanaged Skill**:
A Skill found outside the Skill Library that CC Switch does not yet own. It must be imported before it can receive a managed deployment.
_Avoid_: Project Skill, independent managed copy

**Workspace Unregistration**:
The act of hiding a Project Workspace from active management without changing that project's contents. Minimal workspace identity and deployment history remain archived so surviving links can still be accounted for.
_Avoid_: Workspace deletion, deployment cleanup

**Workspace Relocation**:
The explicit reassociation of an Unavailable Workspace with a new physical directory after the old path has disappeared and the new location passes identity checks. A still-existing old path makes the new directory a distinct Project Workspace.
_Avoid_: Path edit, workspace retargeting

**Workspace Forget**:
The permanent removal of an Archived Workspace identity after every Deployment has been safely removed or explicitly forgotten. The structured activity log retains only the minimal audit event.
_Avoid_: Unregistration, recursive deletion
