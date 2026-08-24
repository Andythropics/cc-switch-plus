//! Shared Skill Deployment domain seam.
//!
//! The module deliberately exposes only `inspect` and `apply` as its behavior
//! boundary.  Consumer paths, symlink validation, and desired-state
//! persistence stay behind that boundary so project workspaces can reuse the
//! same reconciliation semantics later.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::config::get_home_dir;
use crate::database::Database;
#[cfg(target_os = "macos")]
use crate::error::AppError;
#[cfg(target_os = "macos")]
use crate::services::activity::{
    ActivityActor, ActivityBatchContext, ActivityDetailCode, ActivityEventInput, ActivityOperation,
    ActivityOutcome, ActivityReason, ActivityRecorder, ActivityTarget, ActivityTrigger,
};
#[cfg(target_os = "macos")]
use crate::services::deployment_recovery::{
    DeploymentRecoveryDisposition, DeploymentRecoveryService,
};
#[cfg(target_os = "macos")]
use crate::services::project_workspace::{
    add_git_exclude, project_git_exclude_path, project_observation_target_root,
    project_removal_target_root, project_target_root, project_workspace_lifecycle,
    remove_git_exclude, workspace_root_matches_identity, WorkspaceLifecycle,
};
use crate::services::skill::{ConsumerCompatibility, LibrarySkill, LibrarySkillAcquisitionService};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentConsumer {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentTarget {
    pub consumer: DeploymentConsumer,
    pub workspace: WorkspaceKind,
    /// Global deployments use the empty string. Project workspaces will use a
    /// durable workspace identity once that module lands.
    #[serde(default)]
    pub workspace_id: String,
}

impl DeploymentTarget {
    pub fn global(consumer: DeploymentConsumer) -> Self {
        Self {
            consumer,
            workspace: WorkspaceKind::Global,
            workspace_id: String::new(),
        }
    }
}

/// Canonical root for a Global consumer.  Inspection callers use this helper
/// without creating the directory; Deployment remains responsible for
/// creating it only when an explicit mutation is applied.
pub(crate) fn global_target_root(consumer: DeploymentConsumer) -> PathBuf {
    match consumer {
        DeploymentConsumer::Claude => get_home_dir().join(".claude").join("skills"),
        DeploymentConsumer::Codex => get_home_dir().join(".agents").join("skills"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesiredDeployment {
    pub id: String,
    pub library_skill_id: String,
    /// Snapshot of the immutable Library directory identity. Keeping this on
    /// the deployment lets undeploy remain safe even if its DB row is missing.
    pub library_directory: String,
    pub target: DeploymentTarget,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedDeploymentState {
    Missing,
    CorrectLink,
    RedirectedLink,
    BrokenLink,
    InvalidLink,
    OccupiedDirectory,
    OccupiedFile,
    Unreadable,
    LibraryMissing,
    UnrecordedLink,
    InvalidTargetRoot,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDeployment {
    pub state: ObservedDeploymentState,
    pub target_path: String,
    pub expected_target: String,
    pub actual_target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    NotDeployed,
    InSync,
    Drift,
    Conflict,
    Orphaned,
    Blocked,
    Archived,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationLifecycle {
    Archived,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentInspection {
    pub library_skill_id: String,
    pub library_directory: String,
    pub target: DeploymentTarget,
    pub desired: Option<DesiredDeployment>,
    pub observed: ObservedDeployment,
    pub observation_token: String,
    pub status: DeploymentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentQuery {
    #[serde(default)]
    pub consumer: Option<DeploymentConsumer>,
    #[serde(default)]
    pub workspace: Option<WorkspaceKind>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub library_skill_ids: Option<Vec<String>>,
}

impl DeploymentQuery {
    pub fn for_target(target: DeploymentTarget) -> Self {
        Self {
            consumer: Some(target.consumer),
            workspace: Some(target.workspace),
            workspace_id: Some(target.workspace_id),
            library_skill_ids: None,
        }
    }

    fn target(&self) -> DeploymentTarget {
        DeploymentTarget {
            consumer: self.consumer.unwrap_or(DeploymentConsumer::Claude),
            workspace: self.workspace.unwrap_or(WorkspaceKind::Global),
            workspace_id: self.workspace_id.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentInspectionResult {
    pub items: Vec<DeploymentInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "action"
)]
pub enum DeploymentIntent {
    Deploy {
        library_skill_id: String,
        target: DeploymentTarget,
    },
    Undeploy {
        library_skill_id: String,
        target: DeploymentTarget,
    },
    Repair {
        library_skill_id: String,
        target: DeploymentTarget,
        observation_token: String,
    },
    Recover {
        library_skill_id: String,
        target: DeploymentTarget,
        observation_token: String,
        confirmed: bool,
    },
    ReplaceForeignLink {
        library_skill_id: String,
        target: DeploymentTarget,
        observation_token: String,
        confirmed: bool,
    },
    Forget {
        library_skill_id: String,
        target: DeploymentTarget,
    },
}

impl DeploymentIntent {
    fn key(&self) -> (&str, &DeploymentTarget) {
        match self {
            Self::Deploy {
                library_skill_id,
                target,
            }
            | Self::Undeploy {
                library_skill_id,
                target,
            }
            | Self::Repair {
                library_skill_id,
                target,
                ..
            }
            | Self::Recover {
                library_skill_id,
                target,
                ..
            }
            | Self::ReplaceForeignLink {
                library_skill_id,
                target,
                ..
            }
            | Self::Forget {
                library_skill_id,
                target,
            } => (library_skill_id, target),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentBatch {
    pub intents: Vec<DeploymentIntent>,
}

impl DeploymentBatch {
    pub fn single(intent: DeploymentIntent) -> Self {
        Self {
            intents: vec![intent],
        }
    }
}

#[cfg(test)]
mod deployment_intent_serde_tests {
    use super::{DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentTarget};

    #[test]
    fn deserializes_frontend_camel_case_undeploy_batch() {
        let batch: DeploymentBatch = serde_json::from_value(serde_json::json!({
            "intents": [{
                "action": "undeploy",
                "librarySkillId": "library-1",
                "target": {
                    "consumer": "claude",
                    "workspace": "global"
                }
            }]
        }))
        .expect("the Tauri command boundary must accept the frontend DTO");

        assert_eq!(
            batch.intents,
            vec![DeploymentIntent::Undeploy {
                library_skill_id: "library-1".to_string(),
                target: DeploymentTarget::global(DeploymentConsumer::Claude),
            }]
        );
    }

    #[test]
    fn deserializes_every_frontend_camel_case_intent_field() {
        let batch: DeploymentBatch = serde_json::from_value(serde_json::json!({
            "intents": [
                {
                    "action": "deploy",
                    "librarySkillId": "library-1",
                    "target": { "consumer": "claude", "workspace": "global" }
                },
                {
                    "action": "repair",
                    "librarySkillId": "library-1",
                    "target": { "consumer": "claude", "workspace": "global" },
                    "observationToken": "observation-1"
                },
                {
                    "action": "recover",
                    "librarySkillId": "library-1",
                    "target": { "consumer": "claude", "workspace": "global" },
                    "observationToken": "observation-1",
                    "confirmed": true
                },
                {
                    "action": "replaceForeignLink",
                    "librarySkillId": "library-1",
                    "target": { "consumer": "claude", "workspace": "global" },
                    "observationToken": "observation-1",
                    "confirmed": true
                },
                {
                    "action": "forget",
                    "librarySkillId": "library-1",
                    "target": { "consumer": "claude", "workspace": "global" }
                }
            ]
        }))
        .expect("every frontend DeploymentIntent variant must cross the Tauri boundary");

        let target = DeploymentTarget::global(DeploymentConsumer::Claude);
        assert_eq!(
            batch.intents,
            vec![
                DeploymentIntent::Deploy {
                    library_skill_id: "library-1".to_string(),
                    target: target.clone(),
                },
                DeploymentIntent::Repair {
                    library_skill_id: "library-1".to_string(),
                    target: target.clone(),
                    observation_token: "observation-1".to_string(),
                },
                DeploymentIntent::Recover {
                    library_skill_id: "library-1".to_string(),
                    target: target.clone(),
                    observation_token: "observation-1".to_string(),
                    confirmed: true,
                },
                DeploymentIntent::ReplaceForeignLink {
                    library_skill_id: "library-1".to_string(),
                    target: target.clone(),
                    observation_token: "observation-1".to_string(),
                    confirmed: true,
                },
                DeploymentIntent::Forget {
                    library_skill_id: "library-1".to_string(),
                    target,
                },
            ]
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMutationOutcome {
    Applied,
    Replaced,
    AlreadyInSync,
    Removed,
    AlreadyAbsent,
    Conflict,
    Drift,
    Blocked,
    StaleObservation,
    Forgotten,
    RecoveryRequired,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentItemResult {
    pub library_skill_id: String,
    pub target: DeploymentTarget,
    pub outcome: DeploymentMutationOutcome,
    pub message: Option<String>,
    pub inspection: Option<DeploymentInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentBatchResult {
    pub items: Vec<DeploymentItemResult>,
}

static DEPLOYMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
#[cfg(debug_assertions)]
static FORCE_COMPENSATION_FAILURE: AtomicBool = AtomicBool::new(false);
#[cfg(debug_assertions)]
static FORCE_RECOVERY_WORKSPACE_ARCHIVE_BEFORE_COMMIT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct DeploymentRecoveryRequired {
    message: String,
}

fn recovery_required(message: String) -> anyhow::Error {
    DeploymentRecoveryRequired { message }.into()
}

pub struct SkillDeploymentService {
    db: Arc<Database>,
}

impl SkillDeploymentService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Acquire the shared mutation lock for a composite workflow such as
    /// Project Skill Import. Callers must hold this guard while invoking
    /// `apply_one_for_composite`; normal callers continue to use `apply`.
    pub(crate) fn lock_for_composite() -> Result<MutexGuard<'static, ()>> {
        DEPLOYMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub(crate) fn apply_one_for_composite(
        &self,
        intent: &DeploymentIntent,
    ) -> Result<DeploymentItemResult> {
        self.apply_one(intent)
    }

    /// Migration may encounter a proven legacy link already occupying the
    /// official target. This adopts only an exact link to the expected Library
    /// Skill while the caller holds the Deployment lock; it never adopts a
    /// directory, file, foreign link, or redirected link.
    pub(crate) fn adopt_exact_link_for_composite(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentItemResult> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let observed = self.observe(&skill, target)?;
        if observed.state != ObservedDeploymentState::CorrectLink {
            return Ok(self.result(&skill, target, DeploymentMutationOutcome::Conflict, None));
        }
        if self
            .db
            .get_skill_deployment(library_skill_id, target)?
            .is_some()
        {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::AlreadyInSync,
                None,
            ));
        }
        let now = Utc::now().timestamp();
        self.db.save_skill_deployment(&DesiredDeployment {
            id: uuid::Uuid::new_v4().to_string(),
            library_skill_id: skill.id.clone(),
            library_directory: skill.directory.clone(),
            target: target.clone(),
            created_at: now,
            updated_at: now,
        })?;
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Applied, None))
    }

    /// Restore a Deployment row captured before a composite Library deletion.
    /// This deliberately bypasses Active-only lifecycle guards so an Archived
    /// workspace's exact expected link and original desired row can be restored
    /// after a later filesystem/DB failure.
    pub(crate) fn restore_removed_for_composite(
        &self,
        desired: &DesiredDeployment,
    ) -> Result<DeploymentItemResult> {
        let row_exists = self
            .db
            .get_skill_deployment(&desired.library_skill_id, &desired.target)?
            .is_some();
        let skill = self
            .db
            .get_library_skill_by_id(&desired.library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {}", desired.library_skill_id))?;
        if skill.directory != desired.library_directory {
            return Err(anyhow!(
                "Deployment directory identity changed during compensation"
            ));
        }
        let target_path = {
            #[cfg(target_os = "macos")]
            if desired.target.workspace == WorkspaceKind::Project {
                project_removal_target_root(
                    &self.db,
                    &desired.target.workspace_id,
                    desired.target.consumer,
                )?
                .join(&desired.library_directory)
            } else {
                self.target_path(&desired.target, &desired.library_directory)?
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.target_path(&desired.target, &desired.library_directory)?
            }
        };
        let expected = self.library_path(&desired.library_directory)?;
        if !expected.is_dir() {
            return Err(anyhow!(
                "Library source is missing during Deployment compensation"
            ));
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut created_link = false;
        match fs::symlink_metadata(&target_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if fs::read_link(&target_path)? != expected {
                    return Err(anyhow!("Deployment target drifted during compensation"));
                }
            }
            Ok(_) => return Err(anyhow!("Deployment target is occupied during compensation")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(debug_assertions)]
                if FORCE_COMPENSATION_FAILURE.swap(false, Ordering::SeqCst) {
                    return Err(anyhow!("injected compensation failure"));
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&expected, &target_path)?;
                #[cfg(not(unix))]
                return Err(anyhow!("symbolic-link deployment requires a Unix platform"));
                created_link = true;
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(target_os = "macos")]
        let git_exclude_path = if desired.target.workspace == WorkspaceKind::Project {
            let project_root = target_path
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("project Deployment target has no Workspace root"))?;
            match add_git_exclude(
                project_root,
                desired.target.consumer,
                &desired.library_directory,
            ) {
                Ok(path) => path,
                Err(error) => {
                    if created_link {
                        let _ = fs::remove_file(&target_path);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        if row_exists {
            return Ok(DeploymentItemResult {
                library_skill_id: desired.library_skill_id.clone(),
                target: desired.target.clone(),
                outcome: DeploymentMutationOutcome::AlreadyInSync,
                message: None,
                inspection: None,
            });
        }
        if let Err(error) = self.db.save_skill_deployment(desired) {
            let mut compensation = Vec::new();
            if created_link {
                if let Err(compensation_error) = fs::remove_file(&target_path) {
                    compensation.push(compensation_error.to_string());
                }
            }
            #[cfg(target_os = "macos")]
            if let Some(path) = git_exclude_path {
                if let Err(compensation_error) =
                    remove_git_exclude(&path, desired.target.consumer, &desired.library_directory)
                {
                    compensation.push(compensation_error.to_string());
                }
            }
            return if compensation.is_empty() {
                Err(error.into())
            } else {
                Err(recovery_required(format!(
                    "Deployment compensation DB restore failed ({error}); filesystem compensation failed: {}",
                    compensation.join("; ")
                )))
            };
        }
        Ok(DeploymentItemResult {
            library_skill_id: desired.library_skill_id.clone(),
            target: desired.target.clone(),
            outcome: DeploymentMutationOutcome::Applied,
            message: None,
            inspection: None,
        })
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn force_compensation_failure_for_test(enabled: bool) {
        FORCE_COMPENSATION_FAILURE.store(enabled, Ordering::SeqCst);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn force_recovery_workspace_archive_before_commit_for_test(enabled: bool) {
        FORCE_RECOVERY_WORKSPACE_ARCHIVE_BEFORE_COMMIT.store(enabled, Ordering::SeqCst);
    }

    pub fn inspect(&self, query: DeploymentQuery) -> Result<DeploymentInspectionResult> {
        let target = query.target();
        #[cfg(not(target_os = "macos"))]
        {
            return self.inspect_unsupported(&query, target);
        }
        #[cfg(target_os = "macos")]
        Self::ensure_supported_platform()?;
        let library_skills = self.db.list_library_skills()?;
        let desired = self.db.list_skill_deployments()?;
        let requested = query.library_skill_ids.as_ref();
        let mut items = Vec::new();

        for skill in library_skills {
            if requested.is_some_and(|ids| !ids.iter().any(|id| id == &skill.id)) {
                continue;
            }
            let desired_item = desired
                .iter()
                .find(|item| item.library_skill_id == skill.id && item.target == target);
            items.push(self.inspect_skill(&skill, desired_item.cloned(), &target)?);
        }

        // Keep an orphaned desired row visible even when its Library row was
        // deleted or corrupted; inspection must never silently drop intent.
        for row in desired.iter().filter(|row| row.target == target) {
            if requested.is_some_and(|ids| !ids.iter().any(|id| id == &row.library_skill_id)) {
                continue;
            }
            if !items
                .iter()
                .any(|item| item.library_skill_id == row.library_skill_id)
            {
                items.push(self.inspect_missing_library(row, &target)?);
            }
        }
        items.sort_by(|left, right| {
            left.library_directory
                .cmp(&right.library_directory)
                .then(left.library_skill_id.cmp(&right.library_skill_id))
        });
        Ok(DeploymentInspectionResult { items })
    }

    #[cfg(not(target_os = "macos"))]
    fn inspect_unsupported(
        &self,
        query: &DeploymentQuery,
        target: DeploymentTarget,
    ) -> Result<DeploymentInspectionResult> {
        let library_skills = self.db.list_library_skills()?;
        let desired = self.db.list_skill_deployments()?;
        let requested = query.library_skill_ids.as_ref();
        let unsupported = || ObservedDeployment {
            state: ObservedDeploymentState::UnsupportedPlatform,
            target_path: String::new(),
            expected_target: String::new(),
            actual_target: None,
        };
        let mut items = Vec::new();
        for skill in library_skills {
            if requested.is_some_and(|ids| !ids.iter().any(|id| id == &skill.id)) {
                continue;
            }
            let desired_item = desired
                .iter()
                .find(|item| item.library_skill_id == skill.id && item.target == target)
                .cloned();
            items.push(DeploymentInspection {
                library_skill_id: skill.id,
                library_directory: skill.directory,
                target: target.clone(),
                desired: desired_item,
                observed: unsupported(),
                observation_token: "unsupported-platform".to_string(),
                status: DeploymentStatus::Unsupported,
            });
        }
        for row in desired.iter().filter(|row| row.target == target) {
            if requested.is_some_and(|ids| !ids.iter().any(|id| id == &row.library_skill_id)) {
                continue;
            }
            if !items
                .iter()
                .any(|item| item.library_skill_id == row.library_skill_id)
            {
                items.push(DeploymentInspection {
                    library_skill_id: row.library_skill_id.clone(),
                    library_directory: row.library_directory.clone(),
                    target: target.clone(),
                    desired: Some(row.clone()),
                    observed: unsupported(),
                    observation_token: "unsupported-platform".to_string(),
                    status: DeploymentStatus::Unsupported,
                });
            }
        }
        items.sort_by(|left, right| {
            left.library_directory
                .cmp(&right.library_directory)
                .then(left.library_skill_id.cmp(&right.library_skill_id))
        });
        Ok(DeploymentInspectionResult { items })
    }

    pub fn apply(&self, batch: DeploymentBatch) -> Result<DeploymentBatchResult> {
        Self::ensure_supported_platform()?;
        let needs_library_lock = batch
            .intents
            .iter()
            .any(|intent| matches!(intent, DeploymentIntent::Recover { .. }));
        let item_count = u32::try_from(batch.intents.len())
            .map_err(|_| anyhow!("Deployment batch is too large"))?;
        let batch_id = (item_count > 1).then(|| uuid::Uuid::new_v4().to_string());
        let mut seen = HashSet::new();
        for (index, intent) in batch.intents.iter().enumerate() {
            if !seen.insert(intent.key().to_owned()) {
                #[cfg(target_os = "macos")]
                self.record_deployment_activity(
                    intent,
                    ActivityOutcome::Failed,
                    ActivityDetailCode::DuplicateKey,
                    self.existing_deployment_id(intent),
                    Self::batch_context(batch_id.as_deref(), index, item_count),
                );
                return Err(anyhow!(
                    "duplicate Deployment key in batch: {}",
                    intent.key().0
                ));
            }
            if let Err(error) = Self::validate_target(intent.key().1) {
                #[cfg(target_os = "macos")]
                self.record_deployment_activity(
                    intent,
                    ActivityOutcome::Failed,
                    ActivityDetailCode::InvalidInput,
                    self.existing_deployment_id(intent),
                    Self::batch_context(batch_id.as_deref(), index, item_count),
                );
                return Err(error);
            }
        }

        // Composite Library workflows establish Library -> Deployment as the
        // global mutation-lock order. Recovery reads Library physical identity
        // before adopting a Deployment row, so mixed batches containing it
        // must use the same order. Ordinary Deployment-only batches avoid the
        // broader Library lock.
        let _library_guard = needs_library_lock
            .then(LibrarySkillAcquisitionService::lock_for_composite)
            .transpose()?;
        let lock = DEPLOYMENT_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().map_err(|error| anyhow!(error.to_string()))?;
        let mut items = Vec::with_capacity(batch.intents.len());
        for (index, intent) in batch.intents.into_iter().enumerate() {
            let (library_skill_id, target) = intent.key();
            #[cfg(target_os = "macos")]
            let deployment_id = self.existing_deployment_id(&intent);
            let result = self.apply_one(&intent);
            match result {
                Ok(item) => {
                    #[cfg(target_os = "macos")]
                    {
                        let (outcome, detail) = self.activity_for_result(&item);
                        let deployment_id = deployment_id.or_else(|| {
                            self.db
                                .get_skill_deployment(library_skill_id, target)
                                .ok()
                                .flatten()
                                .map(|desired| desired.id)
                        });
                        self.record_deployment_activity(
                            &intent,
                            outcome,
                            detail,
                            deployment_id,
                            Self::batch_context(batch_id.as_deref(), index, item_count),
                        );
                    }
                    items.push(item);
                }
                Err(error) => {
                    let recovery_required =
                        error.downcast_ref::<DeploymentRecoveryRequired>().is_some();
                    #[cfg(target_os = "macos")]
                    let detail = Self::activity_detail_for_error(&error, recovery_required);
                    let inspection = self
                        .inspect(DeploymentQuery {
                            consumer: Some(target.consumer),
                            workspace: Some(target.workspace),
                            workspace_id: Some(target.workspace_id.clone()),
                            library_skill_ids: Some(vec![library_skill_id.to_string()]),
                        })
                        .ok()
                        .and_then(|result| result.items.into_iter().next());
                    items.push(DeploymentItemResult {
                        library_skill_id: library_skill_id.to_string(),
                        target: target.clone(),
                        outcome: if recovery_required {
                            DeploymentMutationOutcome::RecoveryRequired
                        } else {
                            DeploymentMutationOutcome::Error
                        },
                        message: Some(error.to_string()),
                        inspection,
                    });
                    #[cfg(target_os = "macos")]
                    self.record_deployment_activity(
                        &intent,
                        if recovery_required {
                            ActivityOutcome::CompensationFailed
                        } else {
                            ActivityOutcome::Failed
                        },
                        detail,
                        deployment_id,
                        Self::batch_context(batch_id.as_deref(), index, item_count),
                    );
                }
            }
        }
        Ok(DeploymentBatchResult { items })
    }

    #[cfg(target_os = "macos")]
    fn batch_context(
        batch_id: Option<&str>,
        index: usize,
        item_count: u32,
    ) -> Option<ActivityBatchContext> {
        batch_id.map(|batch_id| ActivityBatchContext {
            batch_id: batch_id.to_string(),
            item_index: index as u32,
            item_count,
        })
    }

    #[cfg(target_os = "macos")]
    fn existing_deployment_id(&self, intent: &DeploymentIntent) -> Option<String> {
        let (library_skill_id, target) = intent.key();
        self.db
            .get_skill_deployment(library_skill_id, target)
            .ok()
            .flatten()
            .map(|desired| desired.id)
    }

    #[cfg(target_os = "macos")]
    fn activity_for_result(
        &self,
        item: &DeploymentItemResult,
    ) -> (ActivityOutcome, ActivityDetailCode) {
        match item.outcome {
            DeploymentMutationOutcome::Applied
            | DeploymentMutationOutcome::Replaced
            | DeploymentMutationOutcome::Removed
            | DeploymentMutationOutcome::Forgotten => {
                (ActivityOutcome::Success, ActivityDetailCode::None)
            }
            DeploymentMutationOutcome::AlreadyInSync => {
                (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyInSync)
            }
            DeploymentMutationOutcome::AlreadyAbsent => {
                (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyAbsent)
            }
            DeploymentMutationOutcome::Conflict => (
                ActivityOutcome::Conflict,
                ActivityDetailCode::TargetConflict,
            ),
            DeploymentMutationOutcome::Drift => {
                (ActivityOutcome::Blocked, ActivityDetailCode::Drift)
            }
            DeploymentMutationOutcome::StaleObservation => (
                ActivityOutcome::Blocked,
                ActivityDetailCode::StaleObservation,
            ),
            DeploymentMutationOutcome::Blocked => {
                let detail = item
                    .inspection
                    .as_ref()
                    .map(|inspection| match inspection.observed.state {
                        ObservedDeploymentState::LibraryMissing => {
                            ActivityDetailCode::MissingLibrary
                        }
                        ObservedDeploymentState::OccupiedDirectory
                        | ObservedDeploymentState::OccupiedFile
                        | ObservedDeploymentState::RedirectedLink
                        | ObservedDeploymentState::InvalidTargetRoot => {
                            ActivityDetailCode::TargetConflict
                        }
                        _ => ActivityDetailCode::ValidationFailure,
                    })
                    .unwrap_or_else(|| {
                        if item.target.workspace == WorkspaceKind::Project {
                            match project_workspace_lifecycle(&self.db, &item.target.workspace_id) {
                                Ok(WorkspaceLifecycle::Archived) => {
                                    ActivityDetailCode::ArchivedWorkspace
                                }
                                Ok(WorkspaceLifecycle::Unavailable) => {
                                    ActivityDetailCode::UnavailableWorkspace
                                }
                                _ => ActivityDetailCode::ValidationFailure,
                            }
                        } else {
                            ActivityDetailCode::ValidationFailure
                        }
                    });
                (ActivityOutcome::Blocked, detail)
            }
            DeploymentMutationOutcome::RecoveryRequired => (
                ActivityOutcome::CompensationFailed,
                ActivityDetailCode::CompensationFailure,
            ),
            DeploymentMutationOutcome::Error => (
                ActivityOutcome::Failed,
                ActivityDetailCode::FilesystemFailure,
            ),
        }
    }

    #[cfg(target_os = "macos")]
    fn activity_detail_for_error(
        error: &anyhow::Error,
        recovery_required: bool,
    ) -> ActivityDetailCode {
        if recovery_required {
            return ActivityDetailCode::CompensationFailure;
        }
        if let Some(error) = error.downcast_ref::<AppError>() {
            return match error {
                AppError::Database(_) => ActivityDetailCode::DatabaseFailure,
                AppError::InvalidInput(_) | AppError::Config(_) => ActivityDetailCode::InvalidInput,
                AppError::Io { .. } | AppError::IoContext { .. } => {
                    ActivityDetailCode::FilesystemFailure
                }
                _ => ActivityDetailCode::FilesystemFailure,
            };
        }
        ActivityDetailCode::FilesystemFailure
    }

    #[cfg(target_os = "macos")]
    fn record_deployment_activity(
        &self,
        intent: &DeploymentIntent,
        outcome: ActivityOutcome,
        detail_code: ActivityDetailCode,
        deployment_id: Option<String>,
        batch: Option<ActivityBatchContext>,
    ) {
        let (operation, reason) = match intent {
            DeploymentIntent::Deploy { .. } => {
                (ActivityOperation::Deployment, ActivityReason::Deploy)
            }
            DeploymentIntent::Repair { .. } => (ActivityOperation::Repair, ActivityReason::Repair),
            DeploymentIntent::Recover { .. } => (
                ActivityOperation::Deployment,
                ActivityReason::RecoverDeployment,
            ),
            DeploymentIntent::ReplaceForeignLink { .. } => (
                ActivityOperation::Deployment,
                ActivityReason::ReplaceForeignLink,
            ),
            DeploymentIntent::Undeploy { .. } => {
                (ActivityOperation::Removal, ActivityReason::Undeploy)
            }
            DeploymentIntent::Forget { .. } => {
                (ActivityOperation::Forget, ActivityReason::DeploymentForget)
            }
        };
        let (library_skill_id, target) = intent.key();
        ActivityRecorder::new(self.db.clone()).record_best_effort(ActivityEventInput {
            operation,
            reason,
            outcome,
            actor: ActivityActor::User,
            trigger: if batch.is_some() {
                ActivityTrigger::Batch
            } else {
                ActivityTrigger::Command
            },
            target: ActivityTarget {
                library_skill_id: Some(library_skill_id.to_string()),
                workspace_id: (target.workspace == WorkspaceKind::Project)
                    .then(|| target.workspace_id.clone()),
                deployment_id,
                consumer: Some(target.consumer),
                workspace_kind: Some(target.workspace),
            },
            batch,
            detail_code,
        });
    }

    /// Composite primitives remain silent; their owning top-level workflow
    /// calls this once with the exact typed item it received.
    #[cfg(target_os = "macos")]
    pub(crate) fn record_composite_activity(
        &self,
        intent: &DeploymentIntent,
        result: &Result<DeploymentItemResult>,
    ) {
        match result {
            Ok(item) => {
                let (outcome, detail_code) = self.activity_for_result(item);
                self.record_deployment_activity(
                    intent,
                    outcome,
                    detail_code,
                    item.inspection
                        .as_ref()
                        .and_then(|inspection| inspection.desired.as_ref())
                        .map(|desired| desired.id.clone())
                        .or_else(|| self.existing_deployment_id(intent)),
                    None,
                );
            }
            Err(error) => {
                let recovery_required =
                    error.downcast_ref::<DeploymentRecoveryRequired>().is_some();
                self.record_deployment_activity(
                    intent,
                    if recovery_required {
                        ActivityOutcome::CompensationFailed
                    } else {
                        ActivityOutcome::Failed
                    },
                    Self::activity_detail_for_error(error, recovery_required),
                    self.existing_deployment_id(intent),
                    None,
                );
            }
        }
    }

    fn apply_one(&self, intent: &DeploymentIntent) -> Result<DeploymentItemResult> {
        match intent {
            DeploymentIntent::Deploy {
                library_skill_id,
                target,
            } => self.deploy(library_skill_id, target),
            DeploymentIntent::Repair {
                library_skill_id,
                target,
                observation_token,
            } => self.repair(library_skill_id, target, observation_token),
            DeploymentIntent::Recover {
                library_skill_id,
                target,
                observation_token,
                confirmed,
            } => self.recover(library_skill_id, target, observation_token, *confirmed),
            DeploymentIntent::ReplaceForeignLink {
                library_skill_id,
                target,
                observation_token,
                confirmed,
            } => self.replace_foreign_link(library_skill_id, target, observation_token, *confirmed),
            DeploymentIntent::Undeploy {
                library_skill_id,
                target,
            } => self.undeploy(library_skill_id, target),
            DeploymentIntent::Forget {
                library_skill_id,
                target,
            } => self.forget(library_skill_id, target),
        }
    }

    fn deploy(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentItemResult> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        if let Err(error) = Self::validate_compatibility(&skill, target.consumer) {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some(error.to_string()),
            ));
        }
        if let Some(blocked) = self.workspace_blocked(&skill, target)? {
            return Ok(blocked);
        }
        let desired = self.db.get_skill_deployment(library_skill_id, target)?;
        let observed = self.observe(&skill, target)?;
        match observed.state {
            ObservedDeploymentState::CorrectLink => {
                if desired.is_some() {
                    return Ok(self.result(
                        &skill,
                        target,
                        DeploymentMutationOutcome::AlreadyInSync,
                        None,
                    ));
                }
                // A link that happens to point into the Library is not adopted
                // implicitly. Recovery remains an explicit future workflow.
                return Ok(self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Conflict,
                    Some("an unrecorded Library link requires explicit adoption".to_string()),
                ));
            }
            ObservedDeploymentState::Missing => {}
            ObservedDeploymentState::LibraryMissing => {
                return Ok(self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Blocked,
                    Some("Library source is missing".to_string()),
                ));
            }
            ObservedDeploymentState::OccupiedDirectory
            | ObservedDeploymentState::OccupiedFile
            | ObservedDeploymentState::RedirectedLink
            | ObservedDeploymentState::BrokenLink
            | ObservedDeploymentState::InvalidLink
            | ObservedDeploymentState::Unreadable
            | ObservedDeploymentState::InvalidTargetRoot
            | ObservedDeploymentState::UnrecordedLink
            | ObservedDeploymentState::UnsupportedPlatform => {
                return Ok(self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Conflict,
                    Some(format!("target is {:?}", observed.state)),
                ));
            }
        }

        let target_path = self.target_path(target, &skill.directory)?;
        self.ensure_target_root(&target_path)?;
        let source_path = self.library_path(&skill.directory)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source_path, &target_path).with_context(|| {
            format!(
                "failed to create Deployment link {} -> {}",
                target_path.display(),
                source_path.display()
            )
        })?;
        #[cfg(not(unix))]
        return Err(anyhow!("symbolic-link deployment requires a Unix platform"));

        #[cfg(target_os = "macos")]
        let git_exclude_path = if target.workspace == WorkspaceKind::Project {
            let project_root = target_path
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("project Deployment target has no Workspace root"))?;
            match add_git_exclude(project_root, target.consumer, &skill.directory) {
                Ok(path) => path,
                Err(error) => {
                    return match Self::compensate_remove_file(&target_path) {
                        Ok(()) => Err(error),
                        Err(compensation_error) => Err(recovery_required(format!(
                            "Git exclude update failed ({error}); filesystem compensation failed ({compensation_error})"
                        ))),
                    };
                }
            }
        } else {
            None
        };

        if desired.is_none() {
            let now = Utc::now().timestamp();
            let row = crate::services::skill_deployment::DesiredDeployment {
                id: uuid::Uuid::new_v4().to_string(),
                library_skill_id: skill.id.clone(),
                library_directory: skill.directory.clone(),
                target: target.clone(),
                created_at: now,
                updated_at: now,
            };
            if let Err(error) = self.db.save_skill_deployment(&row) {
                #[cfg(target_os = "macos")]
                let exclude_rollback = git_exclude_path
                    .as_ref()
                    .map(|path| remove_git_exclude(path, target.consumer, &skill.directory));
                let rollback = Self::compensate_remove_file(&target_path);
                if let Err(rollback_error) = rollback {
                    return Err(recovery_required(format!(
                        "database save failed ({error}); filesystem compensation failed ({rollback_error})"
                    )));
                }
                #[cfg(target_os = "macos")]
                if let Some(Err(rollback_error)) = exclude_rollback {
                    return Err(recovery_required(format!(
                        "database save failed ({error}); Git exclude compensation failed ({rollback_error})"
                    )));
                }
                return Err(error.into());
            }
        }
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Applied, None))
    }

    fn repair(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
        observation_token: &str,
    ) -> Result<DeploymentItemResult> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        if let Err(error) = Self::validate_compatibility(&skill, target.consumer) {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some(error.to_string()),
            ));
        }
        let Some(_desired) = self.db.get_skill_deployment(library_skill_id, target)? else {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some("Repair requires an existing desired Deployment".to_string()),
            ));
        };
        if let Some(blocked) = self.workspace_blocked(&skill, target)? {
            return Ok(blocked);
        }
        let fresh = self.inspect_one(library_skill_id, target)?;
        if fresh.observation_token != observation_token {
            return Ok(self.stale_observation(&skill, target, fresh));
        }
        match fresh.observed.state {
            ObservedDeploymentState::Missing => {}
            ObservedDeploymentState::CorrectLink => {
                return Ok(self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::AlreadyInSync,
                    None,
                ));
            }
            ObservedDeploymentState::LibraryMissing => {
                return Ok(self.result_with_inspection(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Blocked,
                    Some("Library source is missing".to_string()),
                    Some(fresh),
                ));
            }
            ObservedDeploymentState::OccupiedDirectory
            | ObservedDeploymentState::OccupiedFile
            | ObservedDeploymentState::RedirectedLink
            | ObservedDeploymentState::BrokenLink
            | ObservedDeploymentState::InvalidLink
            | ObservedDeploymentState::Unreadable
            | ObservedDeploymentState::InvalidTargetRoot
            | ObservedDeploymentState::UnrecordedLink
            | ObservedDeploymentState::UnsupportedPlatform => {
                return Ok(self.result_with_inspection(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Conflict,
                    Some(format!("target is {:?}", fresh.observed.state)),
                    Some(fresh),
                ));
            }
        }

        let target_path = self.target_path(target, &skill.directory)?;
        self.ensure_target_root(&target_path)?;
        let source_path = self.library_path(&skill.directory)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source_path, &target_path).with_context(|| {
            format!(
                "failed to create Deployment repair link {} -> {}",
                target_path.display(),
                source_path.display()
            )
        })?;
        #[cfg(not(unix))]
        return Err(anyhow!("symbolic-link deployment requires a Unix platform"));

        #[cfg(target_os = "macos")]
        if target.workspace == WorkspaceKind::Project {
            let project_root = target_path
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("project Deployment target has no Workspace root"))?;
            if let Err(error) = add_git_exclude(project_root, target.consumer, &skill.directory) {
                return match Self::compensate_remove_file(&target_path) {
                    Ok(()) => Err(error),
                    Err(compensation_error) => Err(recovery_required(format!(
                        "Git exclude update failed ({error}); filesystem compensation failed ({compensation_error})"
                    ))),
                };
            }
        }
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Applied, None))
    }

    #[cfg(target_os = "macos")]
    fn recover(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
        observation_token: &str,
        confirmed: bool,
    ) -> Result<DeploymentItemResult> {
        let Some(skill) = self.db.get_library_skill_by_id(library_skill_id)? else {
            return Ok(DeploymentItemResult {
                library_skill_id: library_skill_id.to_string(),
                target: target.clone(),
                outcome: DeploymentMutationOutcome::StaleObservation,
                message: Some("Library Skill no longer exists; inspect again".to_string()),
                inspection: None,
            });
        };
        if !confirmed {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some("Deployment recovery requires explicit confirmation".to_string()),
            ));
        }
        if self
            .db
            .get_skill_deployment(library_skill_id, target)?
            .is_some()
        {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::AlreadyInSync,
                None,
            ));
        }
        let fresh = DeploymentRecoveryService::new(self.db.clone())
            .inspect_candidate(library_skill_id, target)?;
        if fresh.disposition != DeploymentRecoveryDisposition::Recoverable
            || fresh.observation_token.as_deref() != Some(observation_token)
        {
            let outcome = if matches!(
                fresh.disposition,
                DeploymentRecoveryDisposition::ArchivedWorkspace
                    | DeploymentRecoveryDisposition::UnavailableWorkspace
                    | DeploymentRecoveryDisposition::Incompatible
            ) {
                DeploymentMutationOutcome::Blocked
            } else {
                DeploymentMutationOutcome::StaleObservation
            };
            return Ok(self.result(
                &skill,
                target,
                outcome,
                Some("Deployment recovery observation is stale or no longer safe".to_string()),
            ));
        }

        #[cfg(debug_assertions)]
        if FORCE_RECOVERY_WORKSPACE_ARCHIVE_BEFORE_COMMIT.swap(false, Ordering::SeqCst)
            && target.workspace == WorkspaceKind::Project
        {
            let mut workspace = self
                .db
                .get_project_workspace(&target.workspace_id)?
                .ok_or_else(|| anyhow!("Project Workspace not found: {}", target.workspace_id))?;
            workspace.lifecycle = WorkspaceLifecycle::Archived;
            self.db.update_project_workspace(&workspace)?;
        }

        // Capture and validate the current Project Workspace immediately
        // before the final candidate observation. The same root is then used
        // for the Git exclude mutation, so recovery never validates one
        // Workspace root and writes through another.
        let commit_workspace = if target.workspace == WorkspaceKind::Project {
            let workspace = self
                .db
                .get_project_workspace(&target.workspace_id)?
                .ok_or_else(|| anyhow!("Project Workspace not found: {}", target.workspace_id))?;
            if workspace.lifecycle != WorkspaceLifecycle::Active
                || !workspace_root_matches_identity(&workspace)?
            {
                return Ok(self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::Blocked,
                    Some("Project Workspace changed before recovery commit".to_string()),
                ));
            }
            Some(workspace)
        } else {
            None
        };

        // This is the last read-only gate before the first side effect. It is
        // intentionally repeated after all earlier work so a changed link,
        // Library physical identity, or Workspace lifecycle/root cannot be
        // adopted from a stale proposal.
        let final_candidate = DeploymentRecoveryService::new(self.db.clone())
            .inspect_candidate(library_skill_id, target)?;
        if final_candidate.disposition != DeploymentRecoveryDisposition::Recoverable
            || final_candidate.observation_token.as_deref() != Some(observation_token)
        {
            let outcome = if matches!(
                final_candidate.disposition,
                DeploymentRecoveryDisposition::ArchivedWorkspace
                    | DeploymentRecoveryDisposition::UnavailableWorkspace
                    | DeploymentRecoveryDisposition::Incompatible
            ) {
                DeploymentMutationOutcome::Blocked
            } else {
                DeploymentMutationOutcome::StaleObservation
            };
            return Ok(self.result(
                &skill,
                target,
                outcome,
                Some("Deployment recovery changed before commit; inspect again".to_string()),
            ));
        }

        // The link is already exact. Recovery mutates only desired state and
        // the local Git exclusion required for Project Deployments.
        let exclude_snapshot = if let Some(workspace) = commit_workspace.as_ref() {
            let exclude_path = project_git_exclude_path(&workspace.root_path);
            let previous = exclude_path
                .as_ref()
                .map(|path| match fs::read(path) {
                    Ok(contents) => Ok(Some(contents)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                })
                .transpose()?
                .flatten();
            add_git_exclude(&workspace.root_path, target.consumer, &skill.directory)?;
            exclude_path.map(|path| (path, previous))
        } else {
            None
        };
        let now = Utc::now().timestamp();
        let desired = DesiredDeployment {
            id: uuid::Uuid::new_v4().to_string(),
            library_skill_id: skill.id.clone(),
            library_directory: skill.directory.clone(),
            target: target.clone(),
            created_at: now,
            updated_at: now,
        };
        if let Err(error) = self.db.save_skill_deployment(&desired) {
            if let Some((path, previous)) = exclude_snapshot {
                #[cfg(debug_assertions)]
                if FORCE_COMPENSATION_FAILURE.swap(false, Ordering::SeqCst) {
                    return Err(recovery_required(format!(
                        "Deployment recovery database save failed ({error}); Git exclude compensation failed"
                    )));
                }
                let rollback = match previous {
                    Some(previous) => fs::write(&path, previous),
                    None => match fs::remove_file(&path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(error),
                    },
                };
                if let Err(compensation_error) = rollback {
                    return Err(recovery_required(format!(
                        "Deployment recovery database save failed ({error}); Git exclude compensation failed ({compensation_error})"
                    )));
                }
            }
            return Err(error.into());
        }
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Applied, None))
    }

    #[cfg(not(target_os = "macos"))]
    fn recover(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
        _observation_token: &str,
        _confirmed: bool,
    ) -> Result<DeploymentItemResult> {
        Ok(DeploymentItemResult {
            library_skill_id: library_skill_id.to_string(),
            target: target.clone(),
            outcome: DeploymentMutationOutcome::Blocked,
            message: Some("Deployment recovery is supported on macOS only".to_string()),
            inspection: None,
        })
    }

    fn replace_foreign_link(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
        observation_token: &str,
        confirmed: bool,
    ) -> Result<DeploymentItemResult> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        if !confirmed {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some("foreign-link replacement requires explicit confirmation".to_string()),
            ));
        }
        if let Err(error) = Self::validate_compatibility(&skill, target.consumer) {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some(error.to_string()),
            ));
        }
        if self
            .db
            .get_skill_deployment(library_skill_id, target)?
            .is_none()
        {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some("ReplaceForeignLink requires an existing desired Deployment".to_string()),
            ));
        }
        if let Some(blocked) = self.workspace_blocked(&skill, target)? {
            return Ok(blocked);
        }
        let fresh = self.inspect_one(library_skill_id, target)?;
        if fresh.observation_token != observation_token {
            return Ok(self.stale_observation(&skill, target, fresh));
        }
        if !matches!(
            fresh.observed.state,
            ObservedDeploymentState::RedirectedLink
                | ObservedDeploymentState::BrokenLink
                | ObservedDeploymentState::InvalidLink
        ) {
            return Ok(self.result_with_inspection(
                &skill,
                target,
                DeploymentMutationOutcome::Conflict,
                Some("target is not a replaceable foreign symbolic link".to_string()),
                Some(fresh),
            ));
        }
        let old_target = fresh
            .observed
            .actual_target
            .clone()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("foreign symbolic link has no recorded target"))?;
        let target_path = self.target_path(target, &skill.directory)?;
        let metadata = fs::symlink_metadata(&target_path)?;
        if !metadata.file_type().is_symlink() {
            let current = self.inspect_one(library_skill_id, target)?;
            return Ok(self.stale_observation(&skill, target, current));
        }
        let current_target = fs::read_link(&target_path)?;
        if current_target != old_target {
            let current = self.inspect_one(library_skill_id, target)?;
            return Ok(self.stale_observation(&skill, target, current));
        }
        let expected = self.library_path(&skill.directory)?;
        fs::remove_file(&target_path).with_context(|| {
            format!(
                "failed to remove confirmed foreign Deployment link {}",
                target_path.display()
            )
        })?;
        #[cfg(unix)]
        if let Err(error) = std::os::unix::fs::symlink(&expected, &target_path) {
            return match Self::compensate_symlink(&old_target, &target_path) {
                Ok(()) => Err(error.into()),
                Err(compensation_error) => Err(recovery_required(format!(
                    "replacement link creation failed ({error}); foreign-link compensation failed ({compensation_error})"
                ))),
            };
        }
        #[cfg(not(unix))]
        return Err(anyhow!("symbolic-link deployment requires a Unix platform"));

        #[cfg(target_os = "macos")]
        if target.workspace == WorkspaceKind::Project {
            let project_root = target_path
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("project Deployment target has no Workspace root"))?;
            if let Err(error) = add_git_exclude(project_root, target.consumer, &skill.directory) {
                let rollback = Self::compensate_remove_file(&target_path)
                    .and_then(|()| Self::compensate_symlink(&old_target, &target_path));
                return match rollback {
                    Ok(()) => Err(error),
                    Err(compensation_error) => Err(recovery_required(format!(
                        "Git exclude update failed ({error}); foreign-link compensation failed ({compensation_error})"
                    ))),
                };
            }
        }
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Replaced, None))
    }

    fn undeploy(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentItemResult> {
        let library = self.db.get_library_skill_by_id(library_skill_id)?;
        let desired = self.db.get_skill_deployment(library_skill_id, target)?;
        let Some(desired) = desired else {
            return Ok(if let Some(skill) = library {
                self.result(
                    &skill,
                    target,
                    DeploymentMutationOutcome::AlreadyAbsent,
                    None,
                )
            } else {
                DeploymentItemResult {
                    library_skill_id: library_skill_id.to_string(),
                    target: target.clone(),
                    outcome: DeploymentMutationOutcome::AlreadyAbsent,
                    message: None,
                    inspection: None,
                }
            });
        };
        let target_path = {
            #[cfg(target_os = "macos")]
            if target.workspace == WorkspaceKind::Project {
                match project_workspace_lifecycle(&self.db, &target.workspace_id)? {
                    WorkspaceLifecycle::Unavailable => {
                        return Ok(DeploymentItemResult {
                            library_skill_id: library_skill_id.to_string(),
                            target: target.clone(),
                            outcome: DeploymentMutationOutcome::Blocked,
                            message: Some("Project Workspace is unavailable".to_string()),
                            inspection: None,
                        });
                    }
                    WorkspaceLifecycle::Active | WorkspaceLifecycle::Archived => {
                        match project_removal_target_root(
                            &self.db,
                            &target.workspace_id,
                            target.consumer,
                        ) {
                            Ok(root) => root.join(&desired.library_directory),
                            Err(error) => {
                                return Ok(DeploymentItemResult {
                                    library_skill_id: library_skill_id.to_string(),
                                    target: target.clone(),
                                    outcome: DeploymentMutationOutcome::Blocked,
                                    message: Some(error.to_string()),
                                    inspection: None,
                                });
                            }
                        }
                    }
                }
            } else {
                self.target_path(target, &desired.library_directory)?
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.target_path(target, &desired.library_directory)?
            }
        };
        let expected = self.library_path(&desired.library_directory)?;
        let metadata = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(DeploymentItemResult {
                    library_skill_id: library_skill_id.to_string(),
                    target: target.clone(),
                    outcome: DeploymentMutationOutcome::Drift,
                    message: Some("managed link is missing".to_string()),
                    inspection: None,
                });
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_symlink() {
            return Ok(DeploymentItemResult {
                library_skill_id: library_skill_id.to_string(),
                target: target.clone(),
                outcome: DeploymentMutationOutcome::Conflict,
                message: Some("target is occupied by unmanaged content".to_string()),
                inspection: None,
            });
        }
        let actual = fs::read_link(&target_path)?;
        if !actual.is_absolute() || actual != expected || !expected.exists() {
            return Ok(DeploymentItemResult {
                library_skill_id: library_skill_id.to_string(),
                target: target.clone(),
                outcome: DeploymentMutationOutcome::Drift,
                message: Some(
                    "managed link no longer resolves to the expected Library Skill".to_string(),
                ),
                inspection: None,
            });
        }

        fs::remove_file(&target_path).with_context(|| {
            format!("failed to remove Deployment link {}", target_path.display())
        })?;

        #[cfg(target_os = "macos")]
        let git_exclude_path = if target.workspace == WorkspaceKind::Project {
            let project_root = target_path
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("project Deployment target has no Workspace root"))?;
            match crate::services::project_workspace::project_git_exclude_path(project_root) {
                Some(path) => {
                    if let Err(error) =
                        remove_git_exclude(&path, target.consumer, &desired.library_directory)
                    {
                        #[cfg(unix)]
                        return match Self::compensate_symlink(&expected, &target_path) {
                            Ok(()) => Err(error),
                            Err(compensation_error) => Err(recovery_required(format!(
                                "Git exclude removal failed ({error}); filesystem compensation failed ({compensation_error})"
                            ))),
                        };
                        #[cfg(not(unix))]
                        return Err(error);
                    }
                    Some(path)
                }
                None => None,
            }
        } else {
            None
        };
        if let Err(error) = self.db.delete_skill_deployment(&desired.id) {
            #[cfg(unix)]
            let compensation = Self::compensate_symlink(&expected, &target_path);
            #[cfg(not(unix))]
            let compensation: std::io::Result<()> =
                Err(std::io::Error::other("symbolic links unsupported"));
            if let Err(compensation_error) = compensation {
                return Err(recovery_required(format!(
                    "database deletion failed ({error}); filesystem compensation failed ({compensation_error})"
                )));
            }
            #[cfg(target_os = "macos")]
            if let Some(path) = git_exclude_path {
                if let Err(compensation_error) = add_git_exclude(
                    target_path.parent().and_then(Path::parent).ok_or_else(|| {
                        anyhow!("project Deployment target has no Workspace root")
                    })?,
                    target.consumer,
                    &desired.library_directory,
                ) {
                    return Err(recovery_required(format!(
                        "database deletion failed ({error}); Git exclude compensation failed ({compensation_error})"
                    )));
                }
                let _ = path;
            }
            return Err(error.into());
        }
        Ok(if let Some(skill) = library {
            self.result(&skill, target, DeploymentMutationOutcome::Removed, None)
        } else {
            DeploymentItemResult {
                library_skill_id: library_skill_id.to_string(),
                target: target.clone(),
                outcome: DeploymentMutationOutcome::Removed,
                message: None,
                inspection: None,
            }
        })
    }

    fn forget(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentItemResult> {
        let desired = self.db.get_skill_deployment(library_skill_id, target)?;
        let Some(desired) = desired else {
            return Ok(DeploymentItemResult {
                library_skill_id: library_skill_id.to_string(),
                target: target.clone(),
                outcome: DeploymentMutationOutcome::AlreadyAbsent,
                message: None,
                inspection: None,
            });
        };
        self.db.delete_skill_deployment(&desired.id)?;
        Ok(DeploymentItemResult {
            library_skill_id: library_skill_id.to_string(),
            target: target.clone(),
            outcome: DeploymentMutationOutcome::Forgotten,
            message: None,
            inspection: None,
        })
    }

    fn result(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
        outcome: DeploymentMutationOutcome,
        message: Option<String>,
    ) -> DeploymentItemResult {
        DeploymentItemResult {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
            outcome,
            message,
            inspection: None,
        }
    }

    fn compensate_remove_file(path: &Path) -> std::io::Result<()> {
        #[cfg(debug_assertions)]
        if FORCE_COMPENSATION_FAILURE.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::other("injected compensation failure"));
        }
        fs::remove_file(path)
    }

    #[cfg(unix)]
    fn compensate_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
        #[cfg(debug_assertions)]
        if FORCE_COMPENSATION_FAILURE.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::other("injected compensation failure"));
        }
        std::os::unix::fs::symlink(source, target)
    }

    fn result_with_inspection(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
        outcome: DeploymentMutationOutcome,
        message: Option<String>,
        inspection: Option<DeploymentInspection>,
    ) -> DeploymentItemResult {
        DeploymentItemResult {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
            outcome,
            message,
            inspection,
        }
    }

    fn stale_observation(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
        inspection: DeploymentInspection,
    ) -> DeploymentItemResult {
        self.result_with_inspection(
            skill,
            target,
            DeploymentMutationOutcome::StaleObservation,
            Some("filesystem observation is stale; inspect again before retrying".to_string()),
            Some(inspection),
        )
    }

    fn inspect_one(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentInspection> {
        self.inspect(DeploymentQuery {
            consumer: Some(target.consumer),
            workspace: Some(target.workspace),
            workspace_id: Some(target.workspace_id.clone()),
            library_skill_ids: Some(vec![library_skill_id.to_string()]),
        })?
        .items
        .into_iter()
        .find(|item| item.library_skill_id == library_skill_id)
        .ok_or_else(|| anyhow!("Deployment inspection item not found: {library_skill_id}"))
    }

    fn workspace_blocked(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
    ) -> Result<Option<DeploymentItemResult>> {
        if let Some(message) = self.workspace_block_message(target)? {
            return Ok(Some(self.result(
                skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some(message),
            )));
        }
        Ok(None)
    }

    fn workspace_block_message(&self, target: &DeploymentTarget) -> Result<Option<String>> {
        #[cfg(target_os = "macos")]
        if target.workspace == WorkspaceKind::Project {
            return Ok(
                match project_workspace_lifecycle(&self.db, &target.workspace_id)? {
                    WorkspaceLifecycle::Active => None,
                    WorkspaceLifecycle::Archived => {
                        Some("Project Workspace is archived".to_string())
                    }
                    WorkspaceLifecycle::Unavailable => {
                        Some("Project Workspace is unavailable".to_string())
                    }
                },
            );
        }
        Ok(None)
    }

    fn inspect_skill(
        &self,
        skill: &LibrarySkill,
        desired: Option<DesiredDeployment>,
        target: &DeploymentTarget,
    ) -> Result<DeploymentInspection> {
        let mut observed = self.observe(skill, target)?;
        if desired.is_none() && observed.state == ObservedDeploymentState::CorrectLink {
            observed.state = ObservedDeploymentState::UnrecordedLink;
        }
        #[cfg(target_os = "macos")]
        let lifecycle = if target.workspace == WorkspaceKind::Project {
            match project_workspace_lifecycle(&self.db, &target.workspace_id)? {
                WorkspaceLifecycle::Active => None,
                WorkspaceLifecycle::Archived => Some(ReconciliationLifecycle::Archived),
                WorkspaceLifecycle::Unavailable => Some(ReconciliationLifecycle::Unavailable),
            }
        } else {
            None
        };
        #[cfg(not(target_os = "macos"))]
        let lifecycle: Option<ReconciliationLifecycle> = None;
        let status = Self::status_for(
            desired.is_some(),
            &observed.state,
            lifecycle.as_ref(),
            Self::is_compatible(skill, target.consumer),
        );
        let observation_token = Self::observation_token(&skill.id, target, &observed);
        Ok(DeploymentInspection {
            library_skill_id: skill.id.clone(),
            library_directory: skill.directory.clone(),
            target: target.clone(),
            desired,
            observed,
            observation_token,
            status,
        })
    }

    fn inspect_missing_library(
        &self,
        desired: &DesiredDeployment,
        target: &DeploymentTarget,
    ) -> Result<DeploymentInspection> {
        let target_path = self.observation_target_path(target, &desired.library_directory)?;
        let expected = self.library_path(&desired.library_directory)?;
        #[cfg(target_os = "macos")]
        let lifecycle = if target.workspace == WorkspaceKind::Project {
            match project_workspace_lifecycle(&self.db, &target.workspace_id)? {
                WorkspaceLifecycle::Active => None,
                WorkspaceLifecycle::Archived => Some(ReconciliationLifecycle::Archived),
                WorkspaceLifecycle::Unavailable => Some(ReconciliationLifecycle::Unavailable),
            }
        } else {
            None
        };
        #[cfg(not(target_os = "macos"))]
        let lifecycle: Option<ReconciliationLifecycle> = None;
        let observed = ObservedDeployment {
            state: ObservedDeploymentState::LibraryMissing,
            target_path: target_path.display().to_string(),
            expected_target: expected.display().to_string(),
            actual_target: None,
        };
        let observation_token =
            Self::observation_token(&desired.library_skill_id, target, &observed);
        Ok(DeploymentInspection {
            library_skill_id: desired.library_skill_id.clone(),
            library_directory: desired.library_directory.clone(),
            target: target.clone(),
            desired: Some(desired.clone()),
            observed,
            observation_token,
            status: Self::status_for(
                true,
                &ObservedDeploymentState::LibraryMissing,
                lifecycle.as_ref(),
                true,
            ),
        })
    }

    fn observe(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
    ) -> Result<ObservedDeployment> {
        let target_path = self.observation_target_path(target, &skill.directory)?;
        let expected = self.library_path(&skill.directory)?;
        let expected_string = expected.display().to_string();
        if !expected.is_dir() {
            return Ok(ObservedDeployment {
                state: ObservedDeploymentState::LibraryMissing,
                target_path: target_path.display().to_string(),
                expected_target: expected_string,
                actual_target: None,
            });
        }
        let root = target_path
            .parent()
            .ok_or_else(|| anyhow!("deployment target has no parent"))?;
        match fs::symlink_metadata(root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Ok(ObservedDeployment {
                    state: ObservedDeploymentState::InvalidTargetRoot,
                    target_path: target_path.display().to_string(),
                    expected_target: expected_string,
                    actual_target: None,
                });
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Ok(ObservedDeployment {
                    state: ObservedDeploymentState::Unreadable,
                    target_path: target_path.display().to_string(),
                    expected_target: expected_string,
                    actual_target: None,
                });
            }
            _ => {}
        }
        let metadata = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ObservedDeployment {
                    state: ObservedDeploymentState::Missing,
                    target_path: target_path.display().to_string(),
                    expected_target: expected_string,
                    actual_target: None,
                });
            }
            Err(error) => {
                return Ok(ObservedDeployment {
                    state: ObservedDeploymentState::Unreadable,
                    target_path: target_path.display().to_string(),
                    expected_target: expected_string,
                    actual_target: Some(error.to_string()),
                });
            }
        };
        if !metadata.file_type().is_symlink() {
            let state = if metadata.is_dir() {
                ObservedDeploymentState::OccupiedDirectory
            } else {
                ObservedDeploymentState::OccupiedFile
            };
            return Ok(ObservedDeployment {
                state,
                target_path: target_path.display().to_string(),
                expected_target: expected_string,
                actual_target: None,
            });
        }
        let actual = fs::read_link(&target_path)
            .with_context(|| format!("failed to read Deployment link {}", target_path.display()))?;
        let actual_string = actual.display().to_string();
        let state = if actual.is_absolute() && actual == expected {
            ObservedDeploymentState::CorrectLink
        } else if !actual.is_absolute() {
            ObservedDeploymentState::InvalidLink
        } else if actual.exists() {
            ObservedDeploymentState::RedirectedLink
        } else {
            ObservedDeploymentState::BrokenLink
        };
        Ok(ObservedDeployment {
            state,
            target_path: target_path.display().to_string(),
            expected_target: expected_string,
            actual_target: Some(actual_string),
        })
    }

    fn status_for(
        desired: bool,
        observed: &ObservedDeploymentState,
        lifecycle: Option<&ReconciliationLifecycle>,
        compatible: bool,
    ) -> DeploymentStatus {
        if matches!(lifecycle, Some(ReconciliationLifecycle::Archived)) {
            return DeploymentStatus::Archived;
        }
        if matches!(lifecycle, Some(ReconciliationLifecycle::Unavailable)) {
            return DeploymentStatus::Blocked;
        }
        if !compatible {
            return DeploymentStatus::Blocked;
        }
        match (desired, observed) {
            (false, ObservedDeploymentState::Missing) => DeploymentStatus::NotDeployed,
            (true, ObservedDeploymentState::CorrectLink) => DeploymentStatus::InSync,
            (false, ObservedDeploymentState::CorrectLink) => DeploymentStatus::Orphaned,
            (_, ObservedDeploymentState::OccupiedDirectory)
            | (_, ObservedDeploymentState::OccupiedFile)
            | (_, ObservedDeploymentState::RedirectedLink)
            | (_, ObservedDeploymentState::InvalidTargetRoot) => DeploymentStatus::Conflict,
            (_, ObservedDeploymentState::UnrecordedLink) => DeploymentStatus::Orphaned,
            (_, ObservedDeploymentState::LibraryMissing) => DeploymentStatus::Orphaned,
            (_, ObservedDeploymentState::Unreadable)
            | (_, ObservedDeploymentState::BrokenLink)
            | (_, ObservedDeploymentState::InvalidLink)
            | (true, ObservedDeploymentState::Missing) => DeploymentStatus::Drift,
            (_, ObservedDeploymentState::UnsupportedPlatform) => DeploymentStatus::Unsupported,
        }
    }

    fn observation_token(
        library_skill_id: &str,
        target: &DeploymentTarget,
        observed: &ObservedDeployment,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(library_skill_id.as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(target).unwrap_or_default());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(observed).unwrap_or_default());
        format!("{:x}", hasher.finalize())
    }

    fn library_path(&self, directory: &str) -> Result<PathBuf> {
        Self::validate_directory(directory)?;
        Ok(crate::config::get_app_config_dir()
            .join("skills")
            .join(directory))
    }

    fn target_path(&self, target: &DeploymentTarget, directory: &str) -> Result<PathBuf> {
        Self::validate_target(target)?;
        Self::validate_directory(directory)?;
        let root = match (target.consumer, target.workspace) {
            (consumer, WorkspaceKind::Global) => global_target_root(consumer),
            (_, WorkspaceKind::Project) => {
                #[cfg(target_os = "macos")]
                {
                    project_target_root(&self.db, &target.workspace_id, target.consumer)?
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(anyhow!(
                        "project Deployment targets are supported on macOS only"
                    ));
                }
            }
        };
        Ok(root.join(directory))
    }

    fn observation_target_path(
        &self,
        target: &DeploymentTarget,
        directory: &str,
    ) -> Result<PathBuf> {
        Self::validate_target(target)?;
        Self::validate_directory(directory)?;
        let root = match (target.consumer, target.workspace) {
            (consumer, WorkspaceKind::Global) => global_target_root(consumer),
            (_, WorkspaceKind::Project) => {
                #[cfg(target_os = "macos")]
                {
                    project_observation_target_root(
                        &self.db,
                        &target.workspace_id,
                        target.consumer,
                    )?
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(anyhow!(
                        "project Deployment targets are supported on macOS only"
                    ));
                }
            }
        };
        Ok(root.join(directory))
    }

    fn ensure_target_root(&self, target_path: &Path) -> Result<()> {
        let root = target_path
            .parent()
            .ok_or_else(|| anyhow!("deployment target has no parent"))?;
        match fs::symlink_metadata(root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(anyhow!("deployment target root must be a real directory"))
            }
            Ok(metadata) if !metadata.is_dir() => Err(anyhow!(
                "deployment target root is occupied by a non-directory"
            )),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(root)
                .with_context(|| {
                    format!("failed to create Deployment target root {}", root.display())
                }),
            Err(error) => Err(error.into()),
        }
    }

    fn validate_directory(directory: &str) -> Result<()> {
        let path = Path::new(directory);
        if directory.trim() != directory
            || directory.is_empty()
            || path.components().count() != 1
            || !path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
            || directory == "."
            || directory == ".."
        {
            return Err(anyhow!(
                "Library Skill directory must be one safe path segment: {directory:?}"
            ));
        }
        Ok(())
    }

    fn validate_target(target: &DeploymentTarget) -> Result<()> {
        if !target.workspace_id.is_empty() && target.workspace == WorkspaceKind::Global {
            return Err(anyhow!(
                "global Deployment cannot carry a workspace identity"
            ));
        }
        if target.workspace == WorkspaceKind::Project && target.workspace_id.is_empty() {
            return Err(anyhow!("project Deployment requires a workspace identity"));
        }
        Ok(())
    }

    fn validate_compatibility(skill: &LibrarySkill, consumer: DeploymentConsumer) -> Result<()> {
        let compatibility: &ConsumerCompatibility = match consumer {
            DeploymentConsumer::Claude => &skill.compatibility.claude,
            DeploymentConsumer::Codex => &skill.compatibility.codex,
        };
        if compatibility.compatible {
            Ok(())
        } else {
            Err(anyhow!(
                "Library Skill is incompatible with {:?}: {}",
                consumer,
                compatibility.issues.join("; ")
            ))
        }
    }

    fn is_compatible(skill: &LibrarySkill, consumer: DeploymentConsumer) -> bool {
        match consumer {
            DeploymentConsumer::Claude => skill.compatibility.claude.compatible,
            DeploymentConsumer::Codex => skill.compatibility.codex.compatible,
        }
    }

    fn ensure_supported_platform() -> Result<()> {
        if !cfg!(target_os = "macos") {
            return Err(anyhow!(
                "the redesigned Skill Deployment system is supported on macOS only"
            ));
        }
        Ok(())
    }
}
