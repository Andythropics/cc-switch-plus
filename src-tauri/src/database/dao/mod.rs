//! Data Access Object layer
//!
//! Database access operations for each domain

#[cfg(target_os = "macos")]
pub mod activity;
pub mod failover;
pub mod library_skills;
pub mod mcp;
pub mod profiles;
#[cfg(target_os = "macos")]
pub mod project_workspaces;
pub mod prompts;
pub mod providers;
pub mod providers_seed;
pub mod proxy;
pub mod settings;
pub mod skill_deployments;
pub mod skills;
pub mod stream_check;
pub mod universal_providers;
pub mod usage_rollup;

// 所有 DAO 方法都通过 Database impl 提供，无需单独导出
// 导出 FailoverQueueItem / Profile 供外部使用
pub use failover::FailoverQueueItem;
pub use profiles::Profile;
