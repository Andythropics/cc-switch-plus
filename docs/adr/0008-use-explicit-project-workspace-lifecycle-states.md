# Use Explicit Project Workspace Lifecycle States

Project Workspaces have Active, Archived, or Unavailable lifecycle states. Registration performs a read-only scan and creates consumer Skills roots lazily on first deployment; unregistration archives rather than deletes the workspace, preserving project contents and minimal deployment history.

Relocation is available only when the old path is unavailable and the selected replacement passes repository or worktree identity checks. It preserves the Workspace ID and history, then reconciles deployments at the new physical root. If the old path still exists, the selected directory is registered as a distinct Workspace instead of silently retargeting the original identity.

Registered Workspace roots may not overlap as ancestors or descendants, including non-Git directories. Both supported consumers discover Skills relative to directory ancestry, so overlapping managed roots would make isolation depend on launch location.

An Archived Workspace may be permanently forgotten only after each recorded Deployment has been safely removed or explicitly forgotten. Permanent forgetting removes the retained Workspace identity but leaves a minimal structured activity entry; it never deletes project content.
