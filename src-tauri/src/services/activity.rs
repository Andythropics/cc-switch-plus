//! Structured, device-local Skills activity history.
//!
//! Activity is deliberately a narrow audit seam. Callers provide only typed
//! operation/reason/outcome codes and stable domain identities; arbitrary
//! messages, paths, Skill content, credentials, and payloads cannot enter the
//! persistence API.

use crate::database::Database;
use crate::error::AppError;
use crate::services::skill_deployment::{DeploymentConsumer, WorkspaceKind};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const MAX_IDENTIFIER_BYTES: usize = 128;
pub(crate) const DEFAULT_LIMIT: u32 = 50;
pub(crate) const MAX_LIMIT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityOperation {
    Library,
    Workspace,
    Deployment,
    Repair,
    Migration,
    Forget,
    Removal,
}

impl ActivityOperation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Workspace => "workspace",
            Self::Deployment => "deployment",
            Self::Repair => "repair",
            Self::Migration => "migration",
            Self::Forget => "forget",
            Self::Removal => "removal",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "library" => Self::Library,
            "workspace" => Self::Workspace,
            "deployment" => Self::Deployment,
            "repair" => Self::Repair,
            "migration" => Self::Migration,
            "forget" => Self::Forget,
            "removal" => Self::Removal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityOutcome {
    Success,
    NoOp,
    Blocked,
    Conflict,
    Failed,
    CompensationFailed,
    RolledBack,
}

impl ActivityOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::NoOp => "no_op",
            Self::Blocked => "blocked",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
            Self::CompensationFailed => "compensation_failed",
            Self::RolledBack => "rolled_back",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "success" => Self::Success,
            "no_op" => Self::NoOp,
            "blocked" => Self::Blocked,
            "conflict" => Self::Conflict,
            "failed" => Self::Failed,
            "compensation_failed" => Self::CompensationFailed,
            "rolled_back" => Self::RolledBack,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityActor {
    User,
    System,
    Migration,
}

impl ActivityActor {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
            Self::Migration => "migration",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "user" => Self::User,
            "system" => Self::System,
            "migration" => Self::Migration,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTrigger {
    Command,
    Startup,
    Focus,
    Manual,
    Batch,
    Resume,
}

impl ActivityTrigger {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Startup => "startup",
            Self::Focus => "focus",
            Self::Manual => "manual",
            Self::Batch => "batch",
            Self::Resume => "resume",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "command" => Self::Command,
            "startup" => Self::Startup,
            "focus" => Self::Focus,
            "manual" => Self::Manual,
            "batch" => Self::Batch,
            "resume" => Self::Resume,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityReason {
    Acquire,
    Import,
    MetadataUpdate,
    Update,
    Register,
    Rename,
    Archive,
    Restore,
    Relocate,
    LifecycleRefresh,
    Deploy,
    ReplaceForeignLink,
    Undeploy,
    Repair,
    Migrate,
    MigrateItem,
    Resume,
    DeploymentForget,
    WorkspaceForget,
    LibraryRemove,
    DeploymentRemove,
    LegacyLinkRemove,
    CompensationRestore,
    ImportAndReplace,
    RecoverDeployment,
}

impl ActivityReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Acquire => "acquire",
            Self::Import => "import",
            Self::MetadataUpdate => "metadata_update",
            Self::Update => "update",
            Self::Register => "register",
            Self::Rename => "rename",
            Self::Archive => "archive",
            Self::Restore => "restore",
            Self::Relocate => "relocate",
            Self::LifecycleRefresh => "lifecycle_refresh",
            Self::Deploy => "deploy",
            Self::ReplaceForeignLink => "replace_foreign_link",
            Self::Undeploy => "undeploy",
            Self::Repair => "repair",
            Self::Migrate => "migrate",
            Self::MigrateItem => "migrate_item",
            Self::Resume => "resume",
            Self::DeploymentForget => "deployment_forget",
            Self::WorkspaceForget => "workspace_forget",
            Self::LibraryRemove => "library_remove",
            Self::DeploymentRemove => "deployment_remove",
            Self::LegacyLinkRemove => "legacy_link_remove",
            Self::CompensationRestore => "compensation_restore",
            Self::ImportAndReplace => "import_and_replace",
            Self::RecoverDeployment => "recover_deployment",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "acquire" => Self::Acquire,
            "import" => Self::Import,
            "metadata_update" => Self::MetadataUpdate,
            "update" => Self::Update,
            "register" => Self::Register,
            "rename" => Self::Rename,
            "archive" => Self::Archive,
            "restore" => Self::Restore,
            "relocate" => Self::Relocate,
            "lifecycle_refresh" => Self::LifecycleRefresh,
            "deploy" => Self::Deploy,
            "replace_foreign_link" => Self::ReplaceForeignLink,
            "undeploy" => Self::Undeploy,
            "repair" => Self::Repair,
            "migrate" => Self::Migrate,
            "migrate_item" => Self::MigrateItem,
            "resume" => Self::Resume,
            "deployment_forget" => Self::DeploymentForget,
            "workspace_forget" => Self::WorkspaceForget,
            "library_remove" => Self::LibraryRemove,
            "deployment_remove" => Self::DeploymentRemove,
            "legacy_link_remove" => Self::LegacyLinkRemove,
            "compensation_restore" => Self::CompensationRestore,
            "import_and_replace" => Self::ImportAndReplace,
            "recover_deployment" => Self::RecoverDeployment,
            _ => return None,
        })
    }

    pub(crate) const fn allowed_for(self, operation: ActivityOperation) -> bool {
        match operation {
            ActivityOperation::Library => matches!(
                self,
                Self::Acquire
                    | Self::Import
                    | Self::MetadataUpdate
                    | Self::Update
                    | Self::ImportAndReplace
            ),
            ActivityOperation::Workspace => matches!(
                self,
                Self::Register
                    | Self::Rename
                    | Self::Archive
                    | Self::Restore
                    | Self::Relocate
                    | Self::LifecycleRefresh
            ),
            ActivityOperation::Deployment => {
                matches!(
                    self,
                    Self::Deploy | Self::ReplaceForeignLink | Self::RecoverDeployment
                )
            }
            ActivityOperation::Repair => matches!(self, Self::Repair),
            ActivityOperation::Migration => {
                matches!(self, Self::Migrate | Self::MigrateItem | Self::Resume)
            }
            ActivityOperation::Forget => {
                matches!(self, Self::DeploymentForget | Self::WorkspaceForget)
            }
            ActivityOperation::Removal => matches!(
                self,
                Self::Undeploy
                    | Self::LibraryRemove
                    | Self::DeploymentRemove
                    | Self::LegacyLinkRemove
                    | Self::CompensationRestore
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityDetailCode {
    None,
    AlreadyInSync,
    AlreadyAbsent,
    StaleObservation,
    Drift,
    MissingLibrary,
    ArchivedWorkspace,
    UnavailableWorkspace,
    TargetConflict,
    InvalidInput,
    UnsupportedPlatform,
    ValidationFailure,
    FilesystemFailure,
    DatabaseFailure,
    CompensationFailure,
    DuplicateKey,
    PartialBatch,
}

impl ActivityDetailCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AlreadyInSync => "already_in_sync",
            Self::AlreadyAbsent => "already_absent",
            Self::StaleObservation => "stale_observation",
            Self::Drift => "drift",
            Self::MissingLibrary => "missing_library",
            Self::ArchivedWorkspace => "archived_workspace",
            Self::UnavailableWorkspace => "unavailable_workspace",
            Self::TargetConflict => "target_conflict",
            Self::InvalidInput => "invalid_input",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::ValidationFailure => "validation_failure",
            Self::FilesystemFailure => "filesystem_failure",
            Self::DatabaseFailure => "database_failure",
            Self::CompensationFailure => "compensation_failure",
            Self::DuplicateKey => "duplicate_key",
            Self::PartialBatch => "partial_batch",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Option<Self> {
        Some(match raw {
            "none" => Self::None,
            "already_in_sync" => Self::AlreadyInSync,
            "already_absent" => Self::AlreadyAbsent,
            "stale_observation" => Self::StaleObservation,
            "drift" => Self::Drift,
            "missing_library" => Self::MissingLibrary,
            "archived_workspace" => Self::ArchivedWorkspace,
            "unavailable_workspace" => Self::UnavailableWorkspace,
            "target_conflict" => Self::TargetConflict,
            "invalid_input" => Self::InvalidInput,
            "unsupported_platform" => Self::UnsupportedPlatform,
            "validation_failure" => Self::ValidationFailure,
            "filesystem_failure" => Self::FilesystemFailure,
            "database_failure" => Self::DatabaseFailure,
            "compensation_failure" => Self::CompensationFailure,
            "duplicate_key" => Self::DuplicateKey,
            "partial_batch" => Self::PartialBatch,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<DeploymentConsumer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_kind: Option<WorkspaceKind>,
}

impl ActivityTarget {
    /// Domain operations may fail before an untrusted caller-supplied ID can
    /// be resolved. Preserve the typed outcome while omitting unsafe identity
    /// text instead of dropping the entire best-effort activity row.
    pub(crate) fn sanitized(mut self) -> Self {
        self.library_skill_id = self.library_skill_id.and_then(Self::safe_identifier);
        self.workspace_id = self.workspace_id.and_then(Self::safe_identifier);
        self.deployment_id = self.deployment_id.and_then(Self::safe_identifier);
        self
    }

    pub(crate) fn safe_identifier(identifier: String) -> Option<String> {
        validate_identifier(&identifier)
            .is_ok()
            .then_some(identifier)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityBatchContext {
    pub batch_id: String,
    pub item_index: u32,
    pub item_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEventInput {
    pub operation: ActivityOperation,
    pub reason: ActivityReason,
    pub outcome: ActivityOutcome,
    pub actor: ActivityActor,
    pub trigger: ActivityTrigger,
    pub target: ActivityTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<ActivityBatchContext>,
    pub detail_code: ActivityDetailCode,
}

impl ActivityEventInput {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !self.reason.allowed_for(self.operation) {
            return Err(AppError::Config(format!(
                "activity reason {} is invalid for operation {}",
                self.reason.as_str(),
                self.operation.as_str()
            )));
        }
        for identifier in [
            self.target.library_skill_id.as_deref(),
            self.target.workspace_id.as_deref(),
            self.target.deployment_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_identifier(identifier)?;
        }
        if let Some(batch) = &self.batch {
            validate_identifier(&batch.batch_id)?;
            if batch.item_count == 0 || batch.item_index >= batch.item_count {
                return Err(AppError::Config(
                    "activity batch item index is outside its item count".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRecord {
    pub id: i64,
    pub operation: ActivityOperation,
    pub reason: ActivityReason,
    pub outcome: ActivityOutcome,
    pub actor: ActivityActor,
    pub trigger: ActivityTrigger,
    pub occurred_at: i64,
    pub target: ActivityTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<ActivityBatchContext>,
    pub detail_code: ActivityDetailCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityCursor {
    pub occurred_at: i64,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityQuery {
    #[serde(default)]
    pub operation: Option<ActivityOperation>,
    #[serde(default)]
    pub reason: Option<ActivityReason>,
    #[serde(default)]
    pub outcome: Option<ActivityOutcome>,
    #[serde(default)]
    pub library_skill_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub deployment_id: Option<String>,
    #[serde(default)]
    pub consumer: Option<DeploymentConsumer>,
    #[serde(default)]
    pub workspace_kind: Option<WorkspaceKind>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub until: Option<i64>,
    #[serde(default)]
    pub cursor: Option<ActivityCursor>,
    #[serde(default)]
    pub limit: Option<u32>,
}

impl ActivityQuery {
    fn validate(&self) -> Result<(), AppError> {
        for identifier in [
            self.library_skill_id.as_deref(),
            self.workspace_id.as_deref(),
            self.deployment_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_identifier(identifier)?;
        }
        if self.since.is_some_and(|value| value < 0)
            || self.until.is_some_and(|value| value < 0)
            || matches!((self.since, self.until), (Some(since), Some(until)) if since > until)
        {
            return Err(AppError::InvalidInput(
                "activity time range is invalid".to_string(),
            ));
        }
        if self
            .cursor
            .is_some_and(|cursor| cursor.occurred_at < 0 || cursor.id <= 0)
        {
            return Err(AppError::InvalidInput(
                "activity cursor is invalid".to_string(),
            ));
        }
        if self.limit == Some(0) {
            return Err(AppError::InvalidInput(
                "activity page limit must be positive".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn normalized_limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPage {
    pub entries: Vec<ActivityRecord>,
    pub next_cursor: Option<ActivityCursor>,
    pub has_more: bool,
}

pub struct ActivityRecorder {
    db: Arc<Database>,
}

impl ActivityRecorder {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn record(&self, event: ActivityEventInput) -> Result<i64, AppError> {
        self.record_at(event, Utc::now().timestamp_millis())
    }

    pub fn record_at(&self, event: ActivityEventInput, occurred_at: i64) -> Result<i64, AppError> {
        event.validate()?;
        self.db.insert_skill_activity(&event, occurred_at)
    }

    /// Append an audit row without changing the result of the primary domain
    /// operation. The warning intentionally contains no event payload.
    pub fn record_best_effort(&self, mut event: ActivityEventInput) {
        event.target = event.target.sanitized();
        if let Err(error) = self.record(event) {
            log::warn!(
                "Skills activity append failed: {error_kind}",
                error_kind = error_kind(&error)
            );
        }
    }

    pub fn list(&self, query: ActivityQuery) -> Result<ActivityPage, AppError> {
        query.validate()?;
        self.db.list_skill_activity(&query)
    }
}

fn validate_identifier(identifier: &str) -> Result<(), AppError> {
    if identifier.is_empty()
        || identifier.len() > MAX_IDENTIFIER_BYTES
        || identifier.contains('\0')
        || identifier.contains('/')
        || identifier.contains('\\')
    {
        return Err(AppError::Config(
            "activity target identity is invalid".to_string(),
        ));
    }
    Ok(())
}

fn error_kind(error: &AppError) -> &'static str {
    match error {
        AppError::Config(_) => "config",
        AppError::Database(_) => "database",
        AppError::Io { .. } => "io",
        _ => "other",
    }
}
