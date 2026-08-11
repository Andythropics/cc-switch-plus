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
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::get_home_dir;
use crate::database::Database;
use crate::services::skill::{ConsumerCompatibility, LibrarySkill};

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
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentInspection {
    pub library_skill_id: String,
    pub library_directory: String,
    pub target: DeploymentTarget,
    pub desired: Option<DesiredDeployment>,
    pub observed: ObservedDeployment,
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
#[serde(rename_all = "camelCase", tag = "action")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMutationOutcome {
    Applied,
    AlreadyInSync,
    Removed,
    AlreadyAbsent,
    Conflict,
    Drift,
    Blocked,
    Forgotten,
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

pub struct SkillDeploymentService {
    db: Arc<Database>,
}

impl SkillDeploymentService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn inspect(&self, query: DeploymentQuery) -> Result<DeploymentInspectionResult> {
        Self::ensure_supported_platform()?;
        let target = query.target();
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

    pub fn apply(&self, batch: DeploymentBatch) -> Result<DeploymentBatchResult> {
        Self::ensure_supported_platform()?;
        let mut seen = HashSet::new();
        for intent in &batch.intents {
            if !seen.insert(intent.key().to_owned()) {
                return Err(anyhow!(
                    "duplicate Deployment key in batch: {}",
                    intent.key().0
                ));
            }
            Self::validate_target(intent.key().1)?;
        }

        let lock = DEPLOYMENT_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().map_err(|error| anyhow!(error.to_string()))?;
        let mut items = Vec::with_capacity(batch.intents.len());
        for intent in batch.intents {
            let (library_skill_id, target) = intent.key();
            let result = self.apply_one(&intent);
            match result {
                Ok(item) => items.push(item),
                Err(error) => {
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
                        outcome: DeploymentMutationOutcome::Error,
                        message: Some(error.to_string()),
                        inspection,
                    });
                }
            }
        }
        Ok(DeploymentBatchResult { items })
    }

    fn apply_one(&self, intent: &DeploymentIntent) -> Result<DeploymentItemResult> {
        match intent {
            DeploymentIntent::Deploy {
                library_skill_id,
                target,
            } => self.deploy(library_skill_id, target, false),
            DeploymentIntent::Repair {
                library_skill_id,
                target,
            } => self.deploy(library_skill_id, target, true),
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
        repair: bool,
    ) -> Result<DeploymentItemResult> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        Self::validate_compatibility(&skill, target.consumer)?;
        let desired = self.db.get_skill_deployment(library_skill_id, target)?;
        if repair && desired.is_none() {
            return Ok(self.result(
                &skill,
                target,
                DeploymentMutationOutcome::Blocked,
                Some("Repair requires an existing desired Deployment".to_string()),
            ));
        }
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
                let rollback = fs::remove_file(&target_path);
                if let Err(rollback_error) = rollback {
                    return Err(anyhow!(
                        "database save failed ({error}); filesystem compensation failed ({rollback_error})"
                    ));
                }
                return Err(error.into());
            }
        }
        Ok(self.result(&skill, target, DeploymentMutationOutcome::Applied, None))
    }

    fn undeploy(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentItemResult> {
        let desired = self.db.get_skill_deployment(library_skill_id, target)?;
        let Some(desired) = desired else {
            let library = self.db.get_library_skill_by_id(library_skill_id)?;
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

        let target_path = self.target_path(target, &desired.library_directory)?;
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
        if let Err(error) = self.db.delete_skill_deployment(&desired.id) {
            #[cfg(unix)]
            let compensation = std::os::unix::fs::symlink(&expected, &target_path);
            #[cfg(not(unix))]
            let compensation: std::io::Result<()> =
                Err(std::io::Error::other("symbolic links unsupported"));
            if let Err(compensation_error) = compensation {
                return Err(anyhow!(
                    "database deletion failed ({error}); filesystem compensation failed ({compensation_error})"
                ));
            }
            return Err(error.into());
        }
        let library = self.db.get_library_skill_by_id(library_skill_id)?;
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

    fn inspect_skill(
        &self,
        skill: &LibrarySkill,
        desired: Option<DesiredDeployment>,
        target: &DeploymentTarget,
    ) -> Result<DeploymentInspection> {
        let observed = self.observe(skill, target)?;
        let status = Self::status_for(desired.is_some(), &observed.state);
        Ok(DeploymentInspection {
            library_skill_id: skill.id.clone(),
            library_directory: skill.directory.clone(),
            target: target.clone(),
            desired,
            observed,
            status,
        })
    }

    fn inspect_missing_library(
        &self,
        desired: &DesiredDeployment,
        target: &DeploymentTarget,
    ) -> Result<DeploymentInspection> {
        let target_path = self.target_path(target, &desired.library_directory)?;
        let expected = self.library_path(&desired.library_directory)?;
        Ok(DeploymentInspection {
            library_skill_id: desired.library_skill_id.clone(),
            library_directory: desired.library_directory.clone(),
            target: target.clone(),
            desired: Some(desired.clone()),
            observed: ObservedDeployment {
                state: ObservedDeploymentState::LibraryMissing,
                target_path: target_path.display().to_string(),
                expected_target: expected.display().to_string(),
                actual_target: None,
            },
            status: DeploymentStatus::Orphaned,
        })
    }

    fn observe(
        &self,
        skill: &LibrarySkill,
        target: &DeploymentTarget,
    ) -> Result<ObservedDeployment> {
        let target_path = self.target_path(target, &skill.directory)?;
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

    fn status_for(desired: bool, observed: &ObservedDeploymentState) -> DeploymentStatus {
        match (desired, observed) {
            (false, ObservedDeploymentState::Missing) => DeploymentStatus::NotDeployed,
            (true, ObservedDeploymentState::CorrectLink) => DeploymentStatus::InSync,
            (false, ObservedDeploymentState::CorrectLink) => DeploymentStatus::Orphaned,
            (_, ObservedDeploymentState::OccupiedDirectory)
            | (_, ObservedDeploymentState::OccupiedFile)
            | (_, ObservedDeploymentState::RedirectedLink)
            | (_, ObservedDeploymentState::UnrecordedLink)
            | (_, ObservedDeploymentState::InvalidTargetRoot) => DeploymentStatus::Conflict,
            (_, ObservedDeploymentState::LibraryMissing) => DeploymentStatus::Orphaned,
            (_, ObservedDeploymentState::Unreadable)
            | (_, ObservedDeploymentState::BrokenLink)
            | (true, ObservedDeploymentState::Missing) => DeploymentStatus::Drift,
            (_, ObservedDeploymentState::UnsupportedPlatform) => DeploymentStatus::Unsupported,
        }
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
            (DeploymentConsumer::Claude, WorkspaceKind::Global) => {
                get_home_dir().join(".claude").join("skills")
            }
            (DeploymentConsumer::Codex, WorkspaceKind::Global) => {
                get_home_dir().join(".agents").join("skills")
            }
            (_, WorkspaceKind::Project) => {
                return Err(anyhow!(
                    "project Deployment targets are not available in this slice"
                ));
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

    fn ensure_supported_platform() -> Result<()> {
        if !cfg!(target_os = "macos") {
            return Err(anyhow!(
                "the redesigned Skill Deployment system is supported on macOS only"
            ));
        }
        Ok(())
    }
}
