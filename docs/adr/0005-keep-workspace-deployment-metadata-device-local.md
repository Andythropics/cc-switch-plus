# Keep Workspace Deployment Metadata Device-Local

Project Workspace identities, physical paths, Deployment records, archived workspace history, and local Git exclusions remain device-local and are excluded from WebDAV, S3, and other multi-device synchronization. Only Skill Library contents and portable source metadata participate in sync. Machine paths and observed filesystem state cannot be safely replayed on another device, so each device establishes its own workspaces and deployments.
