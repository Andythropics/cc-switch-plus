# Separate Project Workspaces from Configuration Profiles

CC Switch will model a Project Workspace as an explicitly registered real directory with per-consumer Skill Deployments, independently of a Configuration Profile. Configuration Profiles will no longer capture or apply Skills; legacy Skill snapshot fields are removed during migration rather than converted into workspace state. This boundary allows simultaneously active projects to use different Skills while keeping workspace lifecycle separate from configuration snapshots.
