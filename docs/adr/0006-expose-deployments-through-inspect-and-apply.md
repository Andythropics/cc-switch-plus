# Expose Deployments Through Inspect and Apply

The Skill Deployment Module will expose only `inspect(query)` and `apply(batch)` at its external seam. `inspect` returns desired database state alongside observed filesystem state; `apply` accepts typed `Deploy`, `Undeploy`, `Repair`, and `Forget` intents and returns ordered per-item outcomes. Expected states such as Conflict, Drift, and Blocked are outcomes rather than exceptions, while invalid input, unsupported platforms, and failed compensation remain errors.

Consumer path rules, workspace resolution, symlink operations, SQLite transactions, Git exclusions, reconciliation, locking, and rollback stay behind this seam as internal Adapters. Workspace registration and Skill Import remain separate Modules. Tauri commands may offer convenient deploy or undeploy entry points, but they are thin Adapters over `apply`, and tests exercise behavior through the two-method Interface.
