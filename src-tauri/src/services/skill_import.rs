//! Explicit import of unmanaged, root-level Project Skills into the Library.
//!
//! Inspection is deliberately read-only.  Apply binds to a fresh observation
//! token and derives every filesystem path from the registered Workspace and
//! finding identity; callers never provide an arbitrary source path.

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::database::Database;
use crate::services::project_workspace::{
    workspace_root_matches_identity, ProjectWorkspaceService, WorkspaceLifecycle,
    WorkspaceRootKind, WorkspaceScopeKind, WorkspaceSkillScope,
};
use crate::services::skill::{
    LibrarySkill, LibrarySkillAcquisitionService, LibrarySkillCompatibility, LibrarySkillSource,
    LibrarySourceKind,
};
use crate::services::skill_deployment::{
    DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome, DeploymentTarget,
    SkillDeploymentService, WorkspaceKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillImportScope {
    RootLevel,
    NestedUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectSkillImportValidationStatus {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportValidation {
    pub status: ProjectSkillImportValidationStatus,
    pub issues: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProjectSkillImportLibraryMatch {
    None,
    Identical {
        library_skill_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        directory: Option<String>,
    },
    Different {
        library_skill_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        directory: Option<String>,
    },
}

impl ProjectSkillImportLibraryMatch {
    fn directory(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Identical { directory, .. } | Self::Different { directory, .. } => {
                directory.as_deref()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportGitState {
    pub tracked: bool,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillImportReplaceBlockReason {
    DirectoryIdentityMismatch,
    GitTrackedContent,
    NestedUnsupported,
    InvalidSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportReplaceEligibility {
    pub eligible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ProjectSkillImportReplaceBlockReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectSkillImportDirectoryCollisionKind {
    None,
    Library,
    Invalid,
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportDirectoryCollision {
    pub kind: ProjectSkillImportDirectoryCollisionKind,
    pub requested: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportFinding {
    pub id: String,
    pub consumer: DeploymentConsumer,
    pub scope: ProjectSkillImportScope,
    /// Display-only source location.  Apply re-derives it from the finding.
    pub source_path: String,
    pub directory: String,
    pub validation: ProjectSkillImportValidation,
    pub compatibility: LibrarySkillCompatibility,
    pub library_match: ProjectSkillImportLibraryMatch,
    pub git: ProjectSkillImportGitState,
    pub directory_collision: ProjectSkillImportDirectoryCollision,
    pub replace_eligibility: ProjectSkillImportReplaceEligibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportInspection {
    pub workspace_id: String,
    pub observation_token: String,
    pub findings: Vec<ProjectSkillImportFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillImportMode {
    ImportOnly,
    ImportAndReplace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProjectSkillImportResolution {
    Reuse {
        library_skill_id: String,
    },
    CreateNew {
        directory: String,
        #[serde(default)]
        display_name: Option<String>,
    },
    ReplaceLibrary {
        library_skill_id: String,
        confirmed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportIntent {
    pub workspace_id: String,
    pub finding_id: String,
    pub observation_token: String,
    pub mode: ProjectSkillImportMode,
    pub resolution: ProjectSkillImportResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillImportOutcome {
    Reused,
    Created,
    LibraryReplaced,
    Deployed,
    Blocked,
    Stale,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillImportResult {
    pub finding_id: String,
    pub outcome: ProjectSkillImportOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ProjectSkillImportReplaceBlockReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone)]
struct FindingRecord {
    finding: ProjectSkillImportFinding,
    source: PathBuf,
    content_hash: Option<String>,
}

// Replaced carries the old and new snapshots plus rollback paths. Boxing the
// large variant would complicate the admission/rollback ownership flow; this
// internal enum is short-lived and never crosses the command boundary.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ImportAdmission {
    Existing(crate::services::skill::LibrarySkill),
    Created(crate::services::skill::LibrarySkill),
    Replaced {
        skill: crate::services::skill::LibrarySkill,
        old_skill: crate::services::skill::LibrarySkill,
        destination: PathBuf,
        old_staging: PathBuf,
        rollback_staging: PathBuf,
        backup_root: PathBuf,
    },
}

#[derive(Debug)]
struct RecoverableAdmissionError {
    message: String,
    backup_path: PathBuf,
}

impl Display for RecoverableAdmissionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RecoverableAdmissionError {}

fn recoverable_admission_error(message: impl Into<String>, backup_root: &Path) -> anyhow::Error {
    let message = message.into();
    if backup_root.is_dir() {
        anyhow::Error::new(RecoverableAdmissionError {
            message,
            backup_path: backup_root.to_path_buf(),
        })
    } else {
        anyhow!("{message}")
    }
}

impl ImportAdmission {
    fn skill(&self) -> &crate::services::skill::LibrarySkill {
        match self {
            Self::Existing(skill) | Self::Created(skill) | Self::Replaced { skill, .. } => skill,
        }
    }

    fn result(&self, finding_id: &str) -> ProjectSkillImportResult {
        match self {
            Self::Existing(skill) => ProjectSkillImportResult {
                finding_id: finding_id.to_string(),
                outcome: ProjectSkillImportOutcome::Reused,
                library_skill_id: Some(skill.id.clone()),
                directory: Some(skill.directory.clone()),
                reason: None,
                message: None,
                backup_path: None,
            },
            Self::Created(skill) => ProjectSkillImportResult {
                finding_id: finding_id.to_string(),
                outcome: ProjectSkillImportOutcome::Created,
                library_skill_id: Some(skill.id.clone()),
                directory: Some(skill.directory.clone()),
                reason: None,
                message: None,
                backup_path: None,
            },
            Self::Replaced {
                skill, backup_root, ..
            } => ProjectSkillImportResult {
                finding_id: finding_id.to_string(),
                outcome: ProjectSkillImportOutcome::LibraryReplaced,
                library_skill_id: Some(skill.id.clone()),
                directory: Some(skill.directory.clone()),
                reason: None,
                message: None,
                backup_path: Some(backup_root.to_string_lossy().to_string()),
            },
        }
    }

    fn finish_success(&self) {
        // Replaced snapshots keep their old contents under the managed
        // backup root so a future recovery can restore the previous version.
        // The rolling retention pass removes old roots before a new backup.
    }

    fn backup_path(&self) -> Option<String> {
        match self {
            Self::Replaced { backup_root, .. } => Some(backup_root.to_string_lossy().to_string()),
            Self::Existing(_) | Self::Created(_) => None,
        }
    }
}

pub struct ProjectSkillImportService {
    db: Arc<Database>,
}

impl ProjectSkillImportService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Read-only scan of the registered Workspace's consumer scopes.
    pub fn inspect(&self, workspace_id: &str) -> Result<ProjectSkillImportInspection> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let (_workspace, records, observation_token) = self.current_records(workspace_id)?;
        Ok(ProjectSkillImportInspection {
            workspace_id: workspace_id.to_string(),
            observation_token,
            findings: records.into_iter().map(|record| record.finding).collect(),
        })
    }

    fn current_records(
        &self,
        workspace_id: &str,
    ) -> Result<(
        crate::services::project_workspace::ProjectWorkspace,
        Vec<FindingRecord>,
        String,
    )> {
        let workspace = self
            .db
            .get_project_workspace(workspace_id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {workspace_id}"))?;
        if workspace.lifecycle != WorkspaceLifecycle::Active {
            return Err(anyhow!(
                "Project Workspace is not Active and cannot import Skills"
            ));
        }
        if !workspace_root_matches_identity(&workspace)? {
            return Err(anyhow!(
                "Project Workspace root identity is unavailable; inspect again after restoring it"
            ));
        }
        let scan =
            ProjectWorkspaceService::new(self.db.clone()).inspect_path(&workspace.root_path)?;
        let records = self.inspect_records(
            workspace_id,
            &scan.canonical_root,
            scan.root_kind,
            &scan.scopes,
        )?;
        let mut token_hasher = Sha256::new();
        token_hasher.update(workspace_id.as_bytes());
        for record in &records {
            token_hasher.update(record.finding.id.as_bytes());
            token_hasher.update(serde_json::to_vec(&record.finding)?);
            token_hasher.update(
                record
                    .content_hash
                    .as_deref()
                    .unwrap_or("invalid")
                    .as_bytes(),
            );
        }
        Ok((workspace, records, format!("{:x}", token_hasher.finalize())))
    }

    /// Apply is implemented after the read-only contract is validated.  The
    /// command binds to a fresh scan while holding the same Library lock used
    /// by Git/ZIP acquisition.  Import-and-replace additionally takes the
    /// Deployment lock after it, preserving the global lock order.
    pub fn apply(&self, intent: ProjectSkillImportIntent) -> Result<ProjectSkillImportResult> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let _library_guard = LibrarySkillAcquisitionService::lock_for_composite()?;
        let needs_deployment_lock = intent.mode == ProjectSkillImportMode::ImportAndReplace
            || matches!(
                &intent.resolution,
                ProjectSkillImportResolution::ReplaceLibrary { .. }
            );
        let _deployment_guard = if needs_deployment_lock {
            Some(SkillDeploymentService::lock_for_composite()?)
        } else {
            None
        };

        let (workspace, records, observation_token) = self.current_records(&intent.workspace_id)?;
        if observation_token != intent.observation_token {
            return Ok(ProjectSkillImportResult::stale(
                &intent.finding_id,
                "project Skill changed since the preview; inspect again before retrying",
            ));
        }
        let Some(record) = records
            .into_iter()
            .find(|record| record.finding.id == intent.finding_id)
        else {
            return Ok(ProjectSkillImportResult::blocked(
                &intent.finding_id,
                ProjectSkillImportReplaceBlockReason::InvalidSource,
                "project Skill finding is no longer available",
            ));
        };
        if record.finding.scope == ProjectSkillImportScope::NestedUnsupported {
            return Ok(ProjectSkillImportResult::blocked(
                &intent.finding_id,
                ProjectSkillImportReplaceBlockReason::NestedUnsupported,
                "nested project Skill scopes are reported but cannot be imported",
            ));
        }
        if record.finding.validation.status != ProjectSkillImportValidationStatus::Valid
            || (intent.mode == ProjectSkillImportMode::ImportAndReplace
                && !consumer_compatible(&record.finding.compatibility, record.finding.consumer))
        {
            return Ok(ProjectSkillImportResult::blocked(
                &intent.finding_id,
                ProjectSkillImportReplaceBlockReason::InvalidSource,
                "canonical SKILL.md or consumer compatibility validation failed",
            ));
        }
        if intent.mode == ProjectSkillImportMode::ImportAndReplace {
            if workspace.lifecycle != WorkspaceLifecycle::Active {
                return Ok(ProjectSkillImportResult::blocked_without_reason(
                    &intent.finding_id,
                    "import-and-replace requires an Active Project Workspace",
                ));
            }
            if record.finding.git.tracked {
                return Ok(ProjectSkillImportResult::blocked(
                    &intent.finding_id,
                    ProjectSkillImportReplaceBlockReason::GitTrackedContent,
                    "import-and-replace is blocked while project Skill content is Git-tracked",
                ));
            }
        }

        let admission = match self.admit(&record, &intent.resolution) {
            Ok(admission) => admission,
            Err(error) => {
                let message = error.to_string();
                if let Some(recoverable) = error.downcast_ref::<RecoverableAdmissionError>() {
                    return Ok(
                        ProjectSkillImportResult::recovery_required_without_admission(
                            &intent.finding_id,
                            message,
                            Some(recoverable.backup_path.to_string_lossy().to_string()),
                        ),
                    );
                }
                return Ok(ProjectSkillImportResult::blocked_without_reason(
                    &intent.finding_id,
                    message,
                ));
            }
        };

        let source_metadata = fs::symlink_metadata(&record.source);
        let fresh_source = match source_metadata {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                LibrarySkillAcquisitionService::inspect_source_directory(&record.source)
            }
            Ok(_) => Err(anyhow!(
                "project Skill source is no longer a real directory"
            )),
            Err(error) => Err(error.into()),
        };
        let source_changed = match (record.content_hash.as_deref(), fresh_source) {
            (Some(expected), Ok(actual)) => {
                actual.content_hash != expected || admission.skill().content_hash != expected
            }
            _ => true,
        };
        if source_changed {
            return match self.rollback_admission(&admission) {
                Ok(()) => Ok(ProjectSkillImportResult::stale(
                    &intent.finding_id,
                    "project Skill source changed during admission; inspect again before retrying",
                )),
                Err(error) => Ok(ProjectSkillImportResult::recovery_required(
                    &intent.finding_id,
                    format!(
                        "project Skill source changed during admission; compensation failed: {error}"
                    ),
                    None,
                    &admission,
                )),
            };
        }

        if intent.mode == ProjectSkillImportMode::ImportAndReplace {
            let competing = self
                .db
                .list_skill_deployments()?
                .into_iter()
                .find(|deployment| {
                    deployment.target.workspace == WorkspaceKind::Project
                        && deployment.target.workspace_id == workspace.id
                        && deployment.target.consumer == record.finding.consumer
                        && deployment.library_directory == record.finding.directory
                        && deployment.library_skill_id != admission.skill().id
                });
            if let Some(competing) = competing {
                return match self.rollback_admission(&admission) {
                    Ok(()) => Ok(ProjectSkillImportResult::blocked(
                        &intent.finding_id,
                        ProjectSkillImportReplaceBlockReason::DirectoryIdentityMismatch,
                        format!(
                            "target directory is already desired by another Library Skill ({})",
                            competing.library_skill_id
                        ),
                    )),
                    Err(error) => Ok(ProjectSkillImportResult::recovery_required(
                        &intent.finding_id,
                        format!(
                            "target directory has another desired Library Skill; compensation failed: {error}"
                        ),
                        None,
                        &admission,
                    )),
                };
            }
        }

        if intent.mode == ProjectSkillImportMode::ImportOnly {
            admission.finish_success();
            return Ok(admission.result(&intent.finding_id));
        }

        let target_directory = admission.skill().directory.clone();
        if target_directory != record.finding.directory {
            return match self.rollback_admission(&admission) {
                Ok(()) => Ok(ProjectSkillImportResult::blocked(
                    &intent.finding_id,
                    ProjectSkillImportReplaceBlockReason::DirectoryIdentityMismatch,
                    "Library directory must match the project Skill directory for import-and-replace",
                )),
                Err(error) => Ok(ProjectSkillImportResult::recovery_required(
                    &intent.finding_id,
                    format!(
                        "Library directory identity mismatch; admission compensation failed: {error}"
                    ),
                    None,
                    &admission,
                )),
            };
        }
        if !record.finding.replace_eligibility.eligible {
            let reason = record
                .finding
                .replace_eligibility
                .reason
                .unwrap_or(ProjectSkillImportReplaceBlockReason::InvalidSource);
            return match self.rollback_admission(&admission) {
                Ok(()) => Ok(ProjectSkillImportResult::blocked(
                    &intent.finding_id,
                    reason,
                    "the preview does not permit replacing this project Skill",
                )),
                Err(error) => Ok(ProjectSkillImportResult::recovery_required(
                    &intent.finding_id,
                    format!("replacement blocked; admission compensation failed: {error}"),
                    None,
                    &admission,
                )),
            };
        }

        if intent.mode == ProjectSkillImportMode::ImportAndReplace {
            let stale_after_recheck = |message: String| match self.rollback_admission(&admission) {
                Ok(()) => ProjectSkillImportResult::stale(&intent.finding_id, message),
                Err(error) => ProjectSkillImportResult::recovery_required(
                    &intent.finding_id,
                    format!("{message}; compensation failed: {error}"),
                    None,
                    &admission,
                ),
            };
            match workspace_root_matches_identity(&workspace) {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(stale_after_recheck(
                        "Project Workspace root identity changed during import; inspect again before retrying"
                            .to_string(),
                    ));
                }
                Err(error) => {
                    return Ok(stale_after_recheck(format!(
                        "Project Workspace root identity could not be revalidated: {error}"
                    )));
                }
            }
            let scan = match ProjectWorkspaceService::new(self.db.clone())
                .inspect_path(&workspace.root_path)
            {
                Ok(scan) => scan,
                Err(error) => {
                    return Ok(stale_after_recheck(format!(
                        "Project Workspace root changed during import: {error}"
                    )));
                }
            };
            let fresh_git = git_status(&scan.canonical_root, scan.root_kind, &record.source);
            if fresh_git != record.finding.git || fresh_git.tracked {
                return Ok(stale_after_recheck(
                    "Git tracking state changed during import; inspect again before replacing the project Skill"
                        .to_string(),
                ));
            }
        }

        self.apply_and_replace(&workspace, &record, admission, &intent.finding_id)
    }

    fn admit(
        &self,
        record: &FindingRecord,
        resolution: &ProjectSkillImportResolution,
    ) -> Result<ImportAdmission> {
        match resolution {
            ProjectSkillImportResolution::Reuse { library_skill_id } => {
                let Some(existing) = self.db.get_library_skill_by_id(library_skill_id)? else {
                    return Err(anyhow!("Library Skill not found: {library_skill_id}"));
                };
                let Some(hash) = record.content_hash.as_deref() else {
                    return Err(anyhow!("source content is not valid"));
                };
                if existing.content_hash != hash {
                    return Err(anyhow!(
                        "reuse requires an identical Library Skill content hash"
                    ));
                }
                Ok(ImportAdmission::Existing(existing))
            }
            ProjectSkillImportResolution::CreateNew {
                directory,
                display_name,
            } => {
                let directory = validate_import_directory(directory)?;
                if let Some(existing) = self.db.get_library_skill_by_content_hash(
                    record.content_hash.as_deref().unwrap_or(""),
                )? {
                    return Ok(ImportAdmission::Existing(existing));
                }
                if self
                    .db
                    .get_library_skill_by_directory(&directory)?
                    .is_some()
                    || LibrarySkillAcquisitionService::library_directory_path()
                        .join(&directory)
                        .symlink_metadata()
                        .is_ok()
                {
                    return Err(anyhow!(
                        "Library directory '{directory}' is occupied; choose a readable unique directory"
                    ));
                }
                let skill = LibrarySkillAcquisitionService::acquire_from_directory_locked(
                    &self.db,
                    &record.source,
                    local_import_source(),
                    Some(&directory),
                )?;
                let mut skill = skill;
                if let Some(display_name) = display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    skill.display_name = display_name.to_string();
                    skill.updated_at = Utc::now().timestamp();
                    match self.db.update_library_skill_display_metadata(
                        &skill.id,
                        &skill.display_name,
                        skill.description.as_deref(),
                        skill.updated_at,
                    ) {
                        Ok(Some(updated)) => skill = updated,
                        Ok(None) => {
                            let compensation = self.rollback_created_skill(&skill).err();
                            return Err(match compensation {
                                Some(compensation) => anyhow!(
                                    "Library Skill disappeared during import; compensation failed: {compensation}"
                                ),
                                None => anyhow!("Library Skill disappeared during import"),
                            });
                        }
                        Err(error) => {
                            let compensation = self.rollback_created_skill(&skill).err();
                            return Err(match compensation {
                                Some(compensation) => anyhow!(
                                    "Library metadata update failed ({error}); compensation failed: {compensation}"
                                ),
                                None => anyhow!("Library metadata update failed ({error})"),
                            });
                        }
                    }
                }
                Ok(ImportAdmission::Created(skill))
            }
            ProjectSkillImportResolution::ReplaceLibrary {
                library_skill_id,
                confirmed,
            } => {
                if !confirmed {
                    return Err(anyhow!(
                        "Library replacement requires explicit confirmation"
                    ));
                }
                let Some(existing) = self.db.get_library_skill_by_id(library_skill_id)? else {
                    return Err(anyhow!("Library Skill not found: {library_skill_id}"));
                };
                match &record.finding.library_match {
                    ProjectSkillImportLibraryMatch::Different {
                        library_skill_id: matched,
                        ..
                    } if matched == library_skill_id => {}
                    _ => {
                        return Err(anyhow!(
                            "replacement requires a different-content Library match for the selected identity"
                        ));
                    }
                }
                self.replace_library_snapshot(&existing, record)
            }
        }
    }

    fn replace_library_snapshot(
        &self,
        existing: &LibrarySkill,
        record: &FindingRecord,
    ) -> Result<ImportAdmission> {
        let library_root = LibrarySkillAcquisitionService::library_directory_path();
        fs::create_dir_all(&library_root)?;
        let destination = library_root.join(&existing.directory);
        if !destination.is_dir() || destination.symlink_metadata()?.file_type().is_symlink() {
            return Err(anyhow!(
                "Library Skill snapshot directory is missing or invalid"
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let staging = library_root.join(format!(".import-stage-{id}"));
        let backup_root = create_backup_root()?;
        let old_staging = backup_root.join("library-old");
        let rollback_staging = backup_root.join("library-rollback");
        if let Err(error) =
            LibrarySkillAcquisitionService::copy_tree_preserving_links(&record.source, &staging)
        {
            let staging_cleanup = fs::remove_dir_all(&staging).err();
            let cleanup = fs::remove_dir_all(&backup_root).err();
            return match (staging_cleanup, cleanup) {
                (Some(staging), Some(backup)) => Err(anyhow!(
                    "Library replacement staging failed ({error}); staging cleanup failed ({staging}); backup cleanup failed ({backup})"
                )),
                (Some(staging), None) => Err(anyhow!(
                    "Library replacement staging failed ({error}); staging cleanup failed ({staging})"
                )),
                (None, Some(backup)) => Err(anyhow!(
                    "Library replacement staging failed ({error}); backup cleanup failed ({backup})"
                )),
                (None, None) => Err(error),
            };
        }
        let metadata = match LibrarySkillAcquisitionService::inspect_source_directory(&staging) {
            Ok(metadata) => metadata,
            Err(error) => {
                let cleanup = fs::remove_dir_all(&staging).err();
                return Err(match cleanup {
                    Some(cleanup) => anyhow!(
                        "Library replacement staging validation failed ({error}); staging cleanup failed ({cleanup})"
                    ),
                    None => error,
                });
            }
        };
        if record.content_hash.as_deref() != Some(metadata.content_hash.as_str()) {
            let cleanup = cleanup_paths(&[&staging, &backup_root]);
            return Err(anyhow!(
                "project Skill changed while staging replacement; inspect again before retrying{}",
                if cleanup.is_empty() {
                    String::new()
                } else {
                    format!("; cleanup failed: {}", cleanup.join("; "))
                }
            ));
        }
        for deployment in self.db.list_skill_deployments()? {
            if deployment.library_skill_id != existing.id {
                continue;
            }
            if !consumer_compatible(&metadata.compatibility, deployment.target.consumer) {
                let cleanup = cleanup_paths(&[&staging, &backup_root]);
                return Err(anyhow!(
                    "replacement is incompatible with existing {:?} Deployment{}",
                    deployment.target.consumer,
                    if cleanup.is_empty() {
                        String::new()
                    } else {
                        format!("; cleanup failed: {}", cleanup.join("; "))
                    }
                ));
            }
        }
        if let Err(error) = fs::rename(&destination, &old_staging) {
            let cleanup = fs::remove_dir_all(&staging).err();
            let backup_cleanup = fs::remove_dir_all(&backup_root).err();
            return match (cleanup, backup_cleanup) {
                (Some(cleanup), Some(backup)) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); staging cleanup failed ({cleanup}); backup cleanup failed ({backup})"
                )),
                (Some(cleanup), None) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); staging cleanup failed ({cleanup})"
                )),
                (None, Some(backup)) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); backup cleanup failed ({backup})"
                )),
                (None, None) => Err(error.into()),
            };
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            let restore = fs::rename(&old_staging, &destination).err();
            let cleanup = fs::remove_dir_all(&staging).err();
            let backup_cleanup = if restore.is_none() {
                fs::remove_dir_all(&backup_root).err()
            } else {
                None
            };
            return match (restore, cleanup, backup_cleanup) {
                (Some(restore), Some(cleanup), _) => Err(
                    recoverable_admission_error(
                        format!(
                            "Library snapshot swap failed ({error}); restore failed ({restore}); staging cleanup failed ({cleanup})"
                        ),
                        &backup_root,
                    )
                ),
                (Some(restore), None, _) => Err(
                    recoverable_admission_error(
                        format!("Library snapshot swap failed ({error}); restore failed ({restore})"),
                        &backup_root,
                    )
                ),
                (None, Some(cleanup), Some(backup)) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); staging cleanup failed ({cleanup}); backup cleanup failed ({backup})"
                )),
                (None, Some(cleanup), None) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); staging cleanup failed ({cleanup})"
                )),
                (None, None, Some(backup)) => Err(anyhow!(
                    "Library snapshot swap failed ({error}); backup cleanup failed ({backup})"
                )),
                (None, None, None) => Err(error.into()),
            };
        }
        let mut replacement = existing.clone();
        replacement.display_name = metadata.display_name;
        replacement.description = metadata.description;
        replacement.compatibility = metadata.compatibility;
        replacement.content_hash = metadata.content_hash;
        replacement.source = local_import_source();
        replacement.updated_at = Utc::now().timestamp();
        let updated = match self.db.update_library_skill_snapshot(&replacement) {
            Ok(Some(skill)) => skill,
            Ok(None) => {
                let restore =
                    restore_library_snapshot(&destination, &old_staging, &rollback_staging);
                let cleanup = fs::remove_dir_all(&backup_root).err();
                return match (restore, cleanup) {
                    (Err(restore), _) => Err(
                        recoverable_admission_error(
                            format!(
                                "Library Skill disappeared during replacement; compensation failed: {restore}"
                            ),
                            &backup_root,
                        )
                    ),
                    (Ok(()), Some(cleanup)) => Err(
                        recoverable_admission_error(
                            format!(
                                "Library Skill disappeared during replacement; backup cleanup failed: {cleanup}"
                            ),
                            &backup_root,
                        )
                    ),
                    (Ok(()), None) => Err(anyhow!("Library Skill disappeared during replacement")),
                };
            }
            Err(error) => {
                let restore =
                    restore_library_snapshot(&destination, &old_staging, &rollback_staging);
                let cleanup = fs::remove_dir_all(&backup_root).err();
                return match (restore, cleanup) {
                    (Err(restore), _) => Err(
                        recoverable_admission_error(
                            format!("Library snapshot update failed ({error}); compensation failed: {restore}"),
                            &backup_root,
                        )
                    ),
                    (Ok(()), Some(cleanup)) => Err(
                        recoverable_admission_error(
                            format!("Library snapshot update failed ({error}); backup cleanup failed: {cleanup}"),
                            &backup_root,
                        )
                    ),
                    (Ok(()), None) => Err(error.into()),
                };
            }
        };
        Ok(ImportAdmission::Replaced {
            skill: updated,
            old_skill: existing.clone(),
            destination,
            old_staging,
            rollback_staging,
            backup_root,
        })
    }

    fn rollback_created_skill(&self, skill: &LibrarySkill) -> Result<()> {
        let path = LibrarySkillAcquisitionService::library_directory_path().join(&skill.directory);
        let staging = LibrarySkillAcquisitionService::library_directory_path()
            .join(format!(".import-rollback-{}", uuid::Uuid::new_v4()));
        match path.symlink_metadata() {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "admitted Library snapshot path changed before rollback: {}",
                    path.display()
                ));
            }
            Ok(_) => {
                fs::rename(&path, &staging).map_err(|error| {
                    anyhow!("stage admitted Library snapshot for rollback failed: {error}")
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let deleted = match self.db.delete_library_skill(&skill.id) {
            Ok(deleted) => deleted,
            Err(error) => {
                let had_staging = staging.symlink_metadata().is_ok();
                let restore = had_staging
                    .then(|| fs::rename(&staging, &path).err())
                    .flatten();
                return Err(match (had_staging, restore) {
                            (true, Some(restore)) => anyhow!(
                                "Library row rollback failed ({error}); snapshot restore failed ({restore})"
                            ),
                            (true, None) => anyhow!(
                                "Library row rollback failed ({error}); admitted snapshot and row remain active"
                            ),
                            (false, _) => anyhow!(
                                "Library row rollback failed ({error}); admitted row remains active but snapshot path was already absent"
                            ),
                        });
            }
        };
        if staging.symlink_metadata().is_ok() {
            if let Err(error) = fs::remove_dir_all(&staging) {
                let restore_path = fs::rename(&staging, &path).err();
                let restore_row = if deleted {
                    self.db.save_library_skill(skill).err()
                } else {
                    None
                };
                return Err(match (restore_path, restore_row) {
                            (Some(restore_path), Some(restore_row)) => anyhow!(
                                "Library snapshot cleanup failed ({error}); snapshot restore failed ({restore_path}); row restore failed ({restore_row})"
                            ),
                            (Some(restore_path), None) => anyhow!(
                                "Library snapshot cleanup failed ({error}); snapshot restore failed ({restore_path})"
                            ),
                            (None, Some(restore_row)) => anyhow!(
                                "Library snapshot cleanup failed ({error}); admitted snapshot and row restored, but cleanup remains unresolved ({restore_row})"
                            ),
                            (None, None) => anyhow!(
                                "Library snapshot cleanup failed ({error}); admitted snapshot and row restored"
                            ),
                        });
            }
        }
        Ok(())
    }

    fn rollback_admission(&self, admission: &ImportAdmission) -> Result<()> {
        match admission {
            ImportAdmission::Existing(_) => Ok(()),
            ImportAdmission::Created(skill) => self.rollback_created_skill(skill),
            ImportAdmission::Replaced {
                skill,
                old_skill,
                destination,
                old_staging,
                rollback_staging,
                backup_root: _,
                ..
            } => {
                // Keep the newly admitted snapshot in rollback_staging until
                // the old DB row is durably restored.  If persistence fails,
                // swap the new snapshot back before reporting compensation.
                restore_library_snapshot(destination, old_staging, rollback_staging)?;
                let db_result = self.db.update_library_skill_snapshot(old_skill);
                match db_result {
                    Ok(Some(_)) => {
                        let cleanup = fs::remove_dir_all(rollback_staging).or_else(|error| {
                            (error.kind() == std::io::ErrorKind::NotFound)
                                .then_some(())
                                .ok_or(error)
                        });
                        if let Err(error) = cleanup {
                            return Err(anyhow!(
                                "Library snapshot rollback persisted but backup cleanup failed ({error}); old snapshot remains active"
                            ));
                        }
                        Ok(())
                    }
                    Ok(None) => {
                        let restore =
                            restore_new_snapshot(destination, rollback_staging, old_staging);
                        let row_restore = self.db.save_library_skill(skill).err();
                        Err(match (restore, row_restore) {
                            (Err(restore), Some(row_restore)) => anyhow!(
                                "Library row disappeared during rollback; new snapshot restore failed ({restore}); row restore failed ({row_restore})"
                            ),
                            (Err(restore), None) => anyhow!(
                                "Library row disappeared during rollback; new snapshot restore failed ({restore})"
                            ),
                            (Ok(()), Some(row_restore)) => anyhow!(
                                "Library row disappeared during rollback; new snapshot restored but row restore failed ({row_restore})"
                            ),
                            (Ok(()), None) => anyhow!(
                                "Library row disappeared during rollback; new snapshot and row restored"
                            ),
                        })
                    }
                    Err(error) => {
                        let restore =
                            restore_new_snapshot(destination, rollback_staging, old_staging);
                        Err(match restore {
                            Ok(()) => anyhow!(
                                "Library row rollback failed ({error}); new snapshot and DB state restored"
                            ),
                            Err(restore) => anyhow!(
                                "Library row rollback failed ({error}); new snapshot restore failed ({restore})"
                            ),
                        })
                    }
                }
            }
        }
    }

    fn apply_and_replace(
        &self,
        workspace: &crate::services::project_workspace::ProjectWorkspace,
        record: &FindingRecord,
        admission: ImportAdmission,
        finding_id: &str,
    ) -> Result<ProjectSkillImportResult> {
        let backup_root = match backup_source_for_admission(&record.source, &admission) {
            Ok(path) => path,
            Err(error) => {
                let compensation = self.rollback_admission(&admission).err();
                let compensation_failed = compensation.is_some();
                let backup = admission.backup_path();
                let message = match compensation {
                    Some(compensation) => {
                        format!(
                            "source backup failed ({error}); compensation failed ({compensation})"
                        )
                    }
                    None => {
                        format!("source backup failed ({error}); Library admission rolled back")
                    }
                };
                return Ok(if compensation_failed {
                    ProjectSkillImportResult::recovery_required(
                        finding_id, message, backup, &admission,
                    )
                } else {
                    ProjectSkillImportResult::rolled_back(finding_id, message, backup)
                });
            }
        };
        if let Err(error) = self.revalidate_before_park(workspace, record, &admission, &backup_root)
        {
            let compensation = self.rollback_admission(&admission).err();
            let compensation_failed = compensation.is_some();
            let message = match compensation {
                Some(compensation) => format!(
                    "project Skill changed before replacement; revalidation failed ({error}); compensation failed: {compensation}"
                ),
                None => format!(
                    "project Skill changed before replacement; revalidation failed ({error}); Library admission rolled back"
                ),
            };
            let backup = Some(backup_root.to_string_lossy().to_string());
            return Ok(if compensation_failed {
                ProjectSkillImportResult::recovery_required(finding_id, message, backup, &admission)
            } else {
                ProjectSkillImportResult::stale(finding_id, message)
            });
        }
        let source_parent = record
            .source
            .parent()
            .ok_or_else(|| anyhow!("project Skill source has no parent"))?;
        let parked_source =
            source_parent.join(format!(".cc-switch-import-old-{}", uuid::Uuid::new_v4()));
        if let Err(error) = fs::rename(&record.source, &parked_source) {
            let compensation = self.rollback_admission(&admission).err();
            let compensation_failed = compensation.is_some();
            let message = match compensation {
                Some(compensation) => format!(
                    "park project Skill source failed: {error}; Library compensation failed: {compensation}"
                ),
                None => format!(
                    "park project Skill source failed: {error}; Library admission rolled back"
                ),
            };
            let backup = Some(backup_root.to_string_lossy().to_string());
            return Ok(if compensation_failed {
                ProjectSkillImportResult::recovery_required(finding_id, message, backup, &admission)
            } else {
                ProjectSkillImportResult::rolled_back(finding_id, message, backup)
            });
        }
        let target = DeploymentTarget {
            consumer: record.finding.consumer,
            workspace: WorkspaceKind::Project,
            workspace_id: workspace.id.clone(),
        };
        let deployment = SkillDeploymentService::new(self.db.clone());
        let deploy_result = deployment.apply_one_for_composite(&DeploymentIntent::Deploy {
            library_skill_id: admission.skill().id.clone(),
            target: target.clone(),
        });
        let success = matches!(
            deploy_result,
            Ok(ref item)
                if matches!(
                    item.outcome,
                    DeploymentMutationOutcome::Applied | DeploymentMutationOutcome::AlreadyInSync
                )
        );
        if success {
            if let Err(error) = fs::remove_dir_all(&parked_source) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!(
                        "project Skill import deployed but parked source cleanup failed: {}",
                        error
                    );
                }
            }
            admission.finish_success();
            let mut result = admission.result(finding_id);
            result.outcome = ProjectSkillImportOutcome::Deployed;
            result.backup_path = Some(backup_root.to_string_lossy().to_string());
            return Ok(result);
        }

        let primary = match deploy_result {
            Ok(item) => format!(
                "Deployment returned {:?}: {}",
                item.outcome,
                item.message.unwrap_or_default()
            ),
            Err(error) => error.to_string(),
        };
        let mut compensation_errors = Vec::new();
        let mut link_cleanup_succeeded = true;
        match Self::remove_expected_deployment_link(&record.source, admission.skill()) {
            Ok(false) => {}
            Ok(true) => {}
            Err(error) => {
                link_cleanup_succeeded = false;
                compensation_errors.push(format!(
                    "remove failed expected Deployment link before source restore: {error}"
                ));
            }
        }
        let mut source_restored = false;
        if link_cleanup_succeeded {
            if let Err(error) = fs::rename(&parked_source, &record.source) {
                compensation_errors.push(format!("restore project source failed: {error}"));
            } else {
                source_restored = true;
            }
        }
        if source_restored {
            if let Err(error) = self.rollback_admission(&admission) {
                compensation_errors.push(format!("Library compensation failed: {error}"));
            }
        } else {
            compensation_errors.push(
                "Library compensation skipped while the project target could not be safely restored; recover from backup_path"
                    .to_string(),
            );
        }
        let message = if compensation_errors.is_empty() {
            format!("{primary}; project import rolled back")
        } else {
            format!(
                "{primary}; compensation failed: {}",
                compensation_errors.join("; ")
            )
        };
        let backup = Some(backup_root.to_string_lossy().to_string());
        if compensation_errors.is_empty() {
            Ok(ProjectSkillImportResult::rolled_back(
                finding_id, message, backup,
            ))
        } else {
            Ok(ProjectSkillImportResult::recovery_required(
                finding_id, message, backup, &admission,
            ))
        }
    }

    fn revalidate_before_park(
        &self,
        workspace: &crate::services::project_workspace::ProjectWorkspace,
        record: &FindingRecord,
        admission: &ImportAdmission,
        backup_root: &Path,
    ) -> Result<()> {
        if !workspace_root_matches_identity(workspace)? {
            return Err(anyhow!("Project Workspace root identity changed"));
        }
        let scan =
            ProjectWorkspaceService::new(self.db.clone()).inspect_path(&workspace.root_path)?;
        let metadata = fs::symlink_metadata(&record.source)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "project Skill source is no longer a real directory"
            ));
        }
        let source = LibrarySkillAcquisitionService::inspect_source_directory(&record.source)?;
        if record.content_hash.as_deref() != Some(source.content_hash.as_str())
            || admission.skill().content_hash != source.content_hash
        {
            return Err(anyhow!("project Skill content hash changed"));
        }
        let backup_source = if backup_root.join("project-source").is_dir() {
            backup_root.join("project-source")
        } else {
            backup_root.join("source")
        };
        let backup_hash = LibrarySkillAcquisitionService::compute_library_hash(&backup_source)?;
        if backup_hash != source.content_hash {
            return Err(anyhow!("project Skill backup content hash changed"));
        }
        let fresh_git = git_status(&scan.canonical_root, scan.root_kind, &record.source);
        if fresh_git != record.finding.git || fresh_git.tracked {
            return Err(anyhow!("Git tracking state changed"));
        }
        Ok(())
    }

    fn remove_expected_deployment_link(source: &Path, skill: &LibrarySkill) -> Result<bool> {
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let link = fs::read_link(source)?;
        let resolved = if link.is_absolute() {
            link
        } else {
            source
                .parent()
                .ok_or_else(|| anyhow!("project Skill target has no parent"))?
                .join(link)
        };
        let expected =
            LibrarySkillAcquisitionService::library_directory_path().join(&skill.directory);
        let equivalent = resolved == expected
            || resolved
                .canonicalize()
                .ok()
                .zip(expected.canonicalize().ok())
                .is_some_and(|(actual, expected)| actual == expected);
        if !equivalent {
            return Ok(false);
        }
        fs::remove_file(source)?;
        Ok(true)
    }

    fn inspect_records(
        &self,
        workspace_id: &str,
        workspace_root: &Path,
        root_kind: WorkspaceRootKind,
        scopes: &[WorkspaceSkillScope],
    ) -> Result<Vec<FindingRecord>> {
        let mut records = Vec::new();
        for scope in scopes {
            let consumer = scope.consumer;
            let import_scope = match scope.kind {
                WorkspaceScopeKind::RootLevel => ProjectSkillImportScope::RootLevel,
                WorkspaceScopeKind::NestedUnsupported => ProjectSkillImportScope::NestedUnsupported,
            };
            for directory in &scope.skill_directories {
                let source = scope.path.join(directory);
                match fs::symlink_metadata(&source) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                    _ => continue,
                }
                if !source.join("SKILL.md").is_file() {
                    continue;
                }
                let canonical_source = fs::canonicalize(&source).unwrap_or_else(|_| source.clone());
                let content = LibrarySkillAcquisitionService::inspect_source_directory(&source);
                let (validation, compatibility, content_hash) = match content {
                    Ok(content) => (
                        ProjectSkillImportValidation {
                            status: ProjectSkillImportValidationStatus::Valid,
                            issues: Vec::new(),
                            display_name: Some(content.display_name),
                            description: content.description,
                        },
                        content.compatibility,
                        Some(content.content_hash),
                    ),
                    Err(error) => {
                        let issue = error.to_string();
                        (
                            ProjectSkillImportValidation {
                                status: ProjectSkillImportValidationStatus::Invalid,
                                issues: vec![issue.clone()],
                                display_name: None,
                                description: None,
                            },
                            invalid_compatibility(issue),
                            None,
                        )
                    }
                };
                let git = git_status(workspace_root, root_kind, &canonical_source);
                let library_match = self.library_match(content_hash.as_deref(), directory)?;
                let directory_collision = self.directory_collision(directory)?;
                let mut reason = None;
                if import_scope == ProjectSkillImportScope::NestedUnsupported {
                    reason = Some(ProjectSkillImportReplaceBlockReason::NestedUnsupported);
                } else if validation.status != ProjectSkillImportValidationStatus::Valid {
                    reason = Some(ProjectSkillImportReplaceBlockReason::InvalidSource);
                } else if library_match
                    .directory()
                    .is_some_and(|library_directory| library_directory != directory)
                {
                    reason = Some(ProjectSkillImportReplaceBlockReason::DirectoryIdentityMismatch);
                }
                if import_scope == ProjectSkillImportScope::RootLevel
                    && validation.status == ProjectSkillImportValidationStatus::Valid
                    && !consumer_compatible(&compatibility, consumer)
                {
                    reason = Some(ProjectSkillImportReplaceBlockReason::InvalidSource);
                } else if import_scope == ProjectSkillImportScope::RootLevel && git.tracked {
                    reason = Some(ProjectSkillImportReplaceBlockReason::GitTrackedContent);
                }
                let id = finding_id(workspace_id, consumer, &import_scope, &canonical_source);
                records.push(FindingRecord {
                    finding: ProjectSkillImportFinding {
                        id,
                        consumer,
                        scope: import_scope,
                        source_path: source.to_string_lossy().to_string(),
                        directory: directory.clone(),
                        validation,
                        compatibility,
                        library_match,
                        git,
                        directory_collision,
                        replace_eligibility: ProjectSkillImportReplaceEligibility {
                            eligible: reason.is_none(),
                            reason,
                        },
                    },
                    source,
                    content_hash,
                });
            }
        }
        records.sort_by(|left, right| left.finding.id.cmp(&right.finding.id));
        Ok(records)
    }

    fn library_match(
        &self,
        content_hash: Option<&str>,
        directory: &str,
    ) -> Result<ProjectSkillImportLibraryMatch> {
        if let Some(hash) = content_hash {
            if let Some(skill) = self.db.get_library_skill_by_content_hash(hash)? {
                return Ok(ProjectSkillImportLibraryMatch::Identical {
                    library_skill_id: skill.id,
                    display_name: Some(skill.display_name),
                    directory: Some(skill.directory),
                });
            }
        }
        if let Some(skill) = self.db.get_library_skill_by_directory(directory)? {
            return Ok(ProjectSkillImportLibraryMatch::Different {
                library_skill_id: skill.id,
                display_name: Some(skill.display_name),
                directory: Some(skill.directory),
            });
        }
        Ok(ProjectSkillImportLibraryMatch::None)
    }

    fn directory_collision(&self, directory: &str) -> Result<ProjectSkillImportDirectoryCollision> {
        let requested = directory.to_string();
        let db_collision = self.db.get_library_skill_by_directory(directory)?.is_some();
        let path_collision = LibrarySkillAcquisitionService::library_directory_path()
            .join(directory)
            .symlink_metadata()
            .is_ok();
        let kind = if db_collision {
            ProjectSkillImportDirectoryCollisionKind::Library
        } else if path_collision {
            ProjectSkillImportDirectoryCollisionKind::Reserved
        } else {
            ProjectSkillImportDirectoryCollisionKind::None
        };
        let suggestions = if kind == ProjectSkillImportDirectoryCollisionKind::None {
            Vec::new()
        } else {
            suggest_directories(&self.db, directory)
        };
        Ok(ProjectSkillImportDirectoryCollision {
            kind,
            requested,
            suggestions,
        })
    }
}

impl ProjectSkillImportResult {
    fn blocked(
        finding_id: &str,
        reason: ProjectSkillImportReplaceBlockReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::Blocked,
            library_skill_id: None,
            directory: None,
            reason: Some(reason),
            message: Some(message.into()),
            backup_path: None,
        }
    }

    fn blocked_without_reason(finding_id: &str, message: impl Into<String>) -> Self {
        Self::blocked_with_backup(finding_id, message, None)
    }

    fn blocked_with_backup(
        finding_id: &str,
        message: impl Into<String>,
        backup: Option<String>,
    ) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::Blocked,
            library_skill_id: None,
            directory: None,
            reason: None,
            message: Some(message.into()),
            backup_path: backup,
        }
    }

    fn recovery_required_without_admission(
        finding_id: &str,
        message: impl Into<String>,
        backup: Option<String>,
    ) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::RecoveryRequired,
            library_skill_id: None,
            directory: None,
            reason: None,
            message: Some(message.into()),
            backup_path: backup,
        }
    }

    fn stale(finding_id: &str, message: impl Into<String>) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::Stale,
            library_skill_id: None,
            directory: None,
            reason: None,
            message: Some(message.into()),
            backup_path: None,
        }
    }

    fn rolled_back(finding_id: &str, message: impl Into<String>, backup: Option<String>) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::RolledBack,
            library_skill_id: None,
            directory: None,
            reason: None,
            message: Some(message.into()),
            backup_path: backup,
        }
    }

    fn recovery_required(
        finding_id: &str,
        message: impl Into<String>,
        backup: Option<String>,
        admission: &ImportAdmission,
    ) -> Self {
        Self {
            finding_id: finding_id.to_string(),
            outcome: ProjectSkillImportOutcome::RecoveryRequired,
            library_skill_id: Some(admission.skill().id.clone()),
            directory: Some(admission.skill().directory.clone()),
            reason: None,
            message: Some(message.into()),
            backup_path: backup.or_else(|| admission.backup_path()),
        }
    }
}

fn local_import_source() -> LibrarySkillSource {
    LibrarySkillSource {
        kind: LibrarySourceKind::LocalImport,
        url: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        skill_path: None,
        marketplace: None,
    }
}

fn validate_import_directory(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed != raw
        || trimmed.starts_with('.')
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || Path::new(trimmed).components().count() != 1
    {
        return Err(anyhow!(
            "Library directory must be one readable path segment: {raw:?}"
        ));
    }
    Ok(trimmed.to_string())
}

fn backup_source_for_admission(source: &Path, admission: &ImportAdmission) -> Result<PathBuf> {
    match admission {
        ImportAdmission::Replaced { backup_root, .. } => {
            let destination = backup_root.join("project-source");
            if let Err(error) =
                LibrarySkillAcquisitionService::copy_tree_preserving_links(source, &destination)
            {
                let cleanup = fs::remove_dir_all(&destination).err();
                return Err(match cleanup {
                    Some(cleanup) => {
                        anyhow!("source backup failed ({error}); backup cleanup failed ({cleanup})")
                    }
                    None => error,
                });
            }
            Ok(backup_root.clone())
        }
        ImportAdmission::Existing(_) | ImportAdmission::Created(_) => backup_source(source),
    }
}

fn backup_source(source: &Path) -> Result<PathBuf> {
    let root = create_backup_root()?;
    let destination = root.join("source");
    if let Err(error) =
        LibrarySkillAcquisitionService::copy_tree_preserving_links(source, &destination)
    {
        let cleanup = fs::remove_dir_all(&root).err();
        return Err(match cleanup {
            Some(cleanup) => {
                anyhow!("source backup failed ({error}); backup cleanup failed ({cleanup})")
            }
            None => error,
        });
    }
    Ok(root)
}

fn cleanup_paths(paths: &[&Path]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| match fs::remove_dir_all(path) {
            Ok(()) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(format!("{}: {error}", path.display())),
        })
        .collect()
}

pub(crate) fn create_backup_root() -> Result<PathBuf> {
    let parent = crate::config::get_app_config_dir().join("skill-import-backups");
    fs::create_dir_all(&parent)?;
    let root = parent.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&root)?;
    prune_backup_roots(&parent)?;
    Ok(root)
}

pub(crate) fn prune_backup_roots(parent: &Path) -> Result<()> {
    let mut roots = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata
                .is_dir()
                .then_some((metadata.modified().ok(), entry.path()))
        })
        .collect::<Vec<_>>();
    roots.sort_by_key(|entry| entry.0);
    while roots.len() > 20 {
        let (_, path) = roots.remove(0);
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn restore_library_snapshot(
    destination: &Path,
    old_staging: &Path,
    rollback_staging: &Path,
) -> Result<()> {
    if destination.exists() || destination.symlink_metadata().is_ok() {
        fs::rename(destination, rollback_staging)?;
    }
    if let Err(error) = fs::rename(old_staging, destination) {
        let restore = fs::rename(rollback_staging, destination).err();
        return Err(match restore {
            Some(restore) => anyhow!(
                "Library snapshot restore failed ({error}); new snapshot restore failed ({restore})"
            ),
            None => error.into(),
        });
    }
    Ok(())
}

/// Swap the newly admitted snapshot back into place after a failed attempt to
/// restore the old snapshot.  The caller must have already moved the old
/// snapshot to `destination`; this helper leaves no staging directory behind
/// on success and keeps the new snapshot at `destination`.
fn restore_new_snapshot(
    destination: &Path,
    rollback_staging: &Path,
    old_staging: &Path,
) -> Result<()> {
    fs::rename(destination, old_staging)?;
    if let Err(error) = fs::rename(rollback_staging, destination) {
        let restore = fs::rename(old_staging, destination).err();
        return Err(match restore {
            Some(restore) => anyhow!(
                "new Library snapshot restore failed ({error}); old snapshot restore failed ({restore})"
            ),
            None => error.into(),
        });
    }
    Ok(())
}

fn invalid_compatibility(issue: String) -> LibrarySkillCompatibility {
    let value = crate::services::skill::ConsumerCompatibility {
        compatible: false,
        issues: vec![issue],
    };
    LibrarySkillCompatibility {
        claude: value.clone(),
        codex: value,
    }
}

fn consumer_compatible(
    compatibility: &LibrarySkillCompatibility,
    consumer: DeploymentConsumer,
) -> bool {
    match consumer {
        DeploymentConsumer::Claude => compatibility.claude.compatible,
        DeploymentConsumer::Codex => compatibility.codex.compatible,
    }
}

fn finding_id(
    workspace_id: &str,
    consumer: DeploymentConsumer,
    scope: &ProjectSkillImportScope,
    source: &Path,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(workspace_id.as_bytes());
    hasher.update(serde_json::to_vec(&consumer).unwrap_or_default());
    hasher.update(serde_json::to_vec(scope).unwrap_or_default());
    hasher.update(source.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn git_status(
    root: &Path,
    root_kind: WorkspaceRootKind,
    source: &Path,
) -> ProjectSkillImportGitState {
    let mut nested_repository = contains_nested_git_metadata(source);
    if nested_repository {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<nested repository>".to_string()],
        };
    }
    if root_kind == WorkspaceRootKind::NonGit {
        return ProjectSkillImportGitState {
            tracked: false,
            paths: Vec::new(),
        };
    }
    let mut cursor = source.parent();
    while let Some(path) = cursor {
        if path == root {
            break;
        }
        if path.join(".git").symlink_metadata().is_ok() {
            nested_repository = true;
            break;
        }
        cursor = path.parent();
    }
    if nested_repository {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<nested repository>".to_string()],
        };
    }
    let relative_path = match source.strip_prefix(root) {
        Ok(path) => path,
        Err(_) => {
            return ProjectSkillImportGitState {
                tracked: true,
                paths: vec!["<Git tracking path unavailable>".to_string()],
            };
        }
    };
    let Some(relative) = relative_path.to_str() else {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<Git tracking path is not UTF-8>".to_string()],
        };
    };
    if relative.contains('\\') {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<Git tracking path contains an ambiguous separator>".to_string()],
        };
    }
    let pathspec = format!(":(top,literal){relative}");
    let output = Command::new("git")
        .args([
            "-C",
            &root.display().to_string(),
            "ls-files",
            "-z",
            "--",
            &pathspec,
        ])
        .output();
    let Ok(output) = output else {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<Git tracking status unavailable>".to_string()],
        };
    };
    if !output.status.success() {
        return ProjectSkillImportGitState {
            tracked: true,
            paths: vec!["<Git tracking status unavailable>".to_string()],
        };
    }
    let paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| {
            String::from_utf8(part.to_vec())
                .unwrap_or_else(|_| format!("<non-UTF-8 tracked path ({} bytes)>", part.len()))
        })
        .collect::<Vec<_>>();
    ProjectSkillImportGitState {
        tracked: !paths.is_empty(),
        paths,
    }
}

fn contains_nested_git_metadata(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return true;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git" {
            return true;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return true;
        };
        if metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && contains_nested_git_metadata(&path)
        {
            return true;
        }
    }
    false
}

fn suggest_directories(db: &Database, directory: &str) -> Vec<String> {
    let mut suggestions = Vec::new();
    for suffix in 0..100 {
        let candidate = if suffix == 0 {
            format!("{directory}-import")
        } else {
            format!("{directory}-import-{suffix}")
        };
        let db_used = db
            .get_library_skill_by_directory(&candidate)
            .ok()
            .flatten()
            .is_some();
        let path_used = LibrarySkillAcquisitionService::library_directory_path()
            .join(&candidate)
            .symlink_metadata()
            .is_ok();
        if !db_used && !path_used {
            suggestions.push(candidate);
            if suggestions.len() >= 3 {
                break;
            }
        }
    }
    suggestions
}
