#[cfg(target_os = "macos")]
pub mod activity;
#[cfg(all(test, target_os = "macos"))]
mod activity_tests;
pub mod balance;
pub mod codex_oauth_models;
pub mod coding_plan;
pub mod config;
#[cfg(target_os = "macos")]
pub mod deployment_recovery;
pub mod env_checker;
pub mod env_manager;
pub mod mcp;
pub mod model_fetch;
pub mod model_pricing;
pub mod omo;
pub mod profile;
#[cfg(target_os = "macos")]
pub mod project_workspace;
pub mod prompt;
pub mod provider;
pub mod proxy;
pub mod s3;
pub mod s3_auto_sync;
pub mod s3_sync;
pub mod session_usage;
pub mod session_usage_codex;
pub mod session_usage_gemini;
pub mod session_usage_grokbuild;
pub mod session_usage_opencode;
pub mod skill;
pub mod skill_deployment;
#[cfg(target_os = "macos")]
pub mod skill_import;
#[cfg(target_os = "macos")]
pub mod skill_update;
#[cfg(target_os = "macos")]
pub mod skills_migration;
#[cfg(target_os = "macos")]
pub mod skills_migration_preview;
pub mod speedtest;
pub mod sql_helpers;
pub mod stream_check;
pub mod subscription;
pub mod subscription_grok;
pub mod sync_protocol;
pub mod usage_cache;
pub mod usage_stats;
pub mod webdav;
pub mod webdav_auto_sync;
pub mod webdav_sync;

#[cfg(target_os = "macos")]
pub use activity::{
    ActivityActor, ActivityBatchContext, ActivityCursor, ActivityDetailCode, ActivityEventInput,
    ActivityOperation, ActivityOutcome, ActivityPage, ActivityQuery, ActivityReason,
    ActivityRecord, ActivityRecorder, ActivityTarget, ActivityTrigger,
};
pub use config::ConfigService;
#[cfg(target_os = "macos")]
pub use deployment_recovery::{
    DeploymentRecoveryDisposition, DeploymentRecoveryFinding, DeploymentRecoveryInspectionResult,
    DeploymentRecoveryQuery, DeploymentRecoveryReason, DeploymentRecoveryService,
};
pub use mcp::McpService;
pub use omo::OmoService;
#[cfg(target_os = "macos")]
pub use project_workspace::{
    ProjectWorkspace, ProjectWorkspaceService, WorkspaceLifecycle, WorkspaceRegistration,
    WorkspaceRegistrationScan, WorkspaceRelocation, WorkspaceRelocationOutcome, WorkspaceRootKind,
    WorkspaceScopeKind, WorkspaceSkillScope,
};
pub use prompt::PromptService;
pub use provider::{ProviderService, ProviderSortUpdate, SwitchResult};
pub use proxy::ProxyService;
#[allow(unused_imports)]
pub use skill::{
    ConsumerCompatibility, DiscoverableSkill, LibrarySkill, LibrarySkillAcquisitionService,
    LibrarySkillCompatibility, LibrarySkillSource, LibrarySourceKind, SkillRepo,
    SkillStorageLocation,
};
pub use skill_deployment::{
    DeploymentBatch, DeploymentBatchResult, DeploymentConsumer, DeploymentInspection,
    DeploymentInspectionResult, DeploymentIntent, DeploymentItemResult, DeploymentMutationOutcome,
    DeploymentQuery, DeploymentStatus, DeploymentTarget, DesiredDeployment, ObservedDeployment,
    ObservedDeploymentState, SkillDeploymentService, WorkspaceKind,
};
#[cfg(target_os = "macos")]
pub use skill_import::{
    ProjectSkillImportDirectoryCollision, ProjectSkillImportDirectoryCollisionKind,
    ProjectSkillImportFinding, ProjectSkillImportGitState, ProjectSkillImportInspection,
    ProjectSkillImportIntent, ProjectSkillImportLibraryMatch, ProjectSkillImportMode,
    ProjectSkillImportOutcome, ProjectSkillImportReplaceBlockReason,
    ProjectSkillImportReplaceEligibility, ProjectSkillImportResolution, ProjectSkillImportResult,
    ProjectSkillImportScope, ProjectSkillImportService, ProjectSkillImportValidation,
    ProjectSkillImportValidationStatus,
};
#[cfg(target_os = "macos")]
pub use skill_update::{
    LibrarySkillDeletionAction, LibrarySkillDeletionInspection, LibrarySkillDeletionIntent,
    LibrarySkillDeletionOutcome, LibrarySkillDeletionResult, LibrarySkillDeletionTarget,
    LibrarySkillUpdateApplyIntent, LibrarySkillUpdateApplyOutcome, LibrarySkillUpdateCheck,
    LibrarySkillUpdateCheckOutcome, LibrarySkillUpdateReason, LibrarySkillUpdateResult,
    LibrarySkillUpdateService, UpdateDeploymentImpact,
};
#[cfg(target_os = "macos")]
pub use skills_migration::{
    SkillsMigrationBackupReference, SkillsMigrationExecutionOutcome,
    SkillsMigrationExecutionResult, SkillsMigrationExecutionService, SkillsMigrationIntent,
    SkillsMigrationItemOutcome, SkillsMigrationItemResult, SkillsMigrationProgress,
    SkillsMigrationRestoreIntent,
};
#[cfg(target_os = "macos")]
pub use skills_migration_preview::{
    SkillsMigrationAction, SkillsMigrationBackupPlan, SkillsMigrationDisposition,
    SkillsMigrationInventoryItem, SkillsMigrationInventoryKind, SkillsMigrationInventoryState,
    SkillsMigrationPageMode, SkillsMigrationPlanItem, SkillsMigrationPreflight,
    SkillsMigrationPreviewService, SkillsMigrationReason, SkillsMigrationStatus,
};
pub use speedtest::{EndpointLatency, SpeedtestService};
pub use usage_cache::UsageCache;
#[allow(unused_imports)]
pub use usage_stats::{
    DailyStats, LogFilters, ModelStats, PaginatedLogs, ProviderLimitStatus, ProviderStats,
    RequestLogDetail, UsageSummary, UsageSummaryByApp,
};
