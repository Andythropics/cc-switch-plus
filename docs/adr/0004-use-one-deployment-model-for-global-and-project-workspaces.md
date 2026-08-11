# Use One Deployment Model for Global and Project Workspaces

Global Workspaces and Project Workspaces will use the same explicit Skill Deployment model and filesystem reconciliation engine. Existing per-consumer `enabled_*` flags will migrate to Global Workspace deployment records instead of remaining as a parallel activation system. This increases migration cost now but prevents global and project Skills from developing incompatible ownership, status, and failure semantics.

The database records desired deployments while filesystem inspection records observed state. Missing, redirected, or obstructed links become visible Deployment Drift and require an explicit repair; CC Switch never silently overwrites a real directory. A Library Skill cannot be removed until every recorded deployment is removed or explicitly forgotten, including deployments belonging to archived workspaces.

Generic Repair may recreate a missing managed link or correct a known managed link, but it never replaces a real directory or a foreign symbolic link. A real directory is handled through Skill Import or explicit user filesystem work. Replacing a foreign link is a separate destructive operation with confirmation and a fresh observation check.

Each deployment mutation is atomic by itself, with rollback on partial database or filesystem failure. Batch operations compose those mutations, continue across independent failures, and return per-item outcomes rather than claiming an all-or-nothing transaction across multiple directories. Reconciliation runs when the Skills UI opens, the application regains focus, a mutation completes, or the user refreshes; continuous filesystem watching is outside the first release. Database-loss recovery scans the private Library and proposes, but never automatically adopts, deployment records for links that resolve into it.

Library, Workspace, Deployment, repair, migration, Forget, and removal operations append structured activity entries describing intent and outcome without recording Skill content or credentials.
