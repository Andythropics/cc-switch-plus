//! Library upstream update and destructive lifecycle orchestration.
//!
//! The update service keeps the Library as the single source of truth.  An
//! upstream snapshot is inspected before it can be staged, and every mutation
//! revalidates the observation token while holding the shared Library lock.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::database::Database;
use crate::error::AppError;
use crate::services::activity::{
    ActivityActor, ActivityBatchContext, ActivityDetailCode, ActivityEventInput, ActivityOperation,
    ActivityOutcome, ActivityReason, ActivityRecorder, ActivityTarget, ActivityTrigger,
};
use crate::services::skill::{
    LibrarySkill, LibrarySkillAcquisitionService, LibrarySkillCompatibility, LibrarySkillSource,
    LibrarySourceKind,
};
use crate::services::skill_deployment::{
    DeploymentInspection, DeploymentItemResult, DeploymentMutationOutcome, DeploymentQuery,
    DeploymentTarget, DesiredDeployment, SkillDeploymentService,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySkillUpdateCheckOutcome {
    UpdateAvailable,
    UpToDate,
    NotUpdatable,
    InvalidCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeploymentImpact {
    pub inspection: DeploymentInspection,
    pub current_compatible: bool,
    pub staged_compatible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillUpdateCheck {
    pub library_skill_id: String,
    pub outcome: LibrarySkillUpdateCheckOutcome,
    pub observation_token: String,
    pub stage_token: Option<String>,
    pub recorded_content_hash: String,
    pub live_content_hash: Option<String>,
    pub staged_content_hash: Option<String>,
    pub local_modified: bool,
    pub compatibility: Option<LibrarySkillCompatibility>,
    pub affected_deployments: Vec<UpdateDeploymentImpact>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillUpdateApplyIntent {
    pub library_skill_id: String,
    pub stage_token: String,
    pub observation_token: String,
    pub confirm_local_modifications: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySkillUpdateReason {
    LocalModificationConfirmationRequired,
    CompatibilityRegression,
    NotUpdatable,
    InvalidCandidate,
    DuplicateContent,
    MissingStage,
    StaleObservation,
    CompensationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySkillUpdateApplyOutcome {
    Updated,
    UpToDate,
    Blocked,
    Stale,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillUpdateResult {
    pub outcome: LibrarySkillUpdateApplyOutcome,
    pub library_skill_id: String,
    pub reason: Option<LibrarySkillUpdateReason>,
    pub recorded_content_hash: Option<String>,
    pub live_content_hash: Option<String>,
    pub staged_content_hash: Option<String>,
    pub affected_deployments: Vec<UpdateDeploymentImpact>,
    pub backup_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillDeletionInspection {
    pub library_skill_id: String,
    pub observation_token: String,
    pub targets: Vec<LibrarySkillDeletionTarget>,
    pub blocked: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySkillDeletionAction {
    RemoveExpectedLink,
    Forget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillDeletionTarget {
    pub inspection: DeploymentInspection,
    pub action_required: LibrarySkillDeletionAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillDeletionIntent {
    pub library_skill_id: String,
    pub observation_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillDeletionResult {
    pub outcome: LibrarySkillDeletionOutcome,
    pub library_skill_id: String,
    pub items: Vec<DeploymentItemResult>,
    pub backup_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySkillDeletionOutcome {
    Deleted,
    Blocked,
    Stale,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageManifest {
    library_skill_id: String,
    observation_token: String,
    upstream_hash: String,
    compatibility: LibrarySkillCompatibility,
    #[serde(default)]
    source: Option<LibrarySkillSource>,
    #[serde(default)]
    external_observation: Option<(PathBuf, String)>,
    #[serde(default)]
    external_candidate_id: Option<String>,
}

enum LibraryDeletionSnapshot {
    Present(String),
    Missing,
    Unsafe,
}

/// Shared updater for private Library snapshots.
pub struct LibrarySkillUpdateService;

#[cfg(debug_assertions)]
static FORCE_ATOMIC_SWAP_FAILURE: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static FORCE_DELETION_RENAME_FAILURE: AtomicBool = AtomicBool::new(false);

impl LibrarySkillUpdateService {
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn force_atomic_swap_failure_for_test(enabled: bool) {
        FORCE_ATOMIC_SWAP_FAILURE.store(enabled, Ordering::SeqCst);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn force_deletion_rename_failure_for_test(enabled: bool) {
        FORCE_DELETION_RENAME_FAILURE.store(enabled, Ordering::SeqCst);
    }

    /// Inspect a checked-out repository fixture.  Production commands obtain
    /// the fixture from the persisted Git/marketplace origin before calling
    /// this seam; keeping the snapshot seam public makes real-filesystem tests
    /// deterministic without mocking HTTP.
    pub fn inspect_from_repository_snapshot(
        db: &Arc<Database>,
        library_skill_id: &str,
        repository_root: &Path,
    ) -> Result<LibrarySkillUpdateCheck> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let skill = db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let current_path = Self::library_path(&skill)?;
        let current_hash = match fs::symlink_metadata(&current_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Some(
                LibrarySkillAcquisitionService::compute_library_hash(&current_path)?,
            ),
            Ok(_) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let mut candidate_error = None;
        let metadata = if matches!(
            skill.source.kind,
            LibrarySourceKind::Zip | LibrarySourceKind::LocalImport
        ) {
            None
        } else {
            match Self::snapshot_path(&skill, repository_root).and_then(|snapshot| {
                LibrarySkillAcquisitionService::inspect_source_directory(&snapshot)
            }) {
                Ok(metadata) => Some(metadata),
                Err(error) => {
                    candidate_error = Some(error.to_string());
                    None
                }
            }
        };
        let upstream_hash = metadata.as_ref().map(|value| value.content_hash.clone());
        let staged_compatibility = metadata.as_ref().map(|value| value.compatibility.clone());
        let impacts = Self::deployment_impacts(db, &skill, staged_compatibility.as_ref())?;
        let observation_token = Self::observation_token(
            &skill,
            current_hash.as_deref(),
            upstream_hash.as_deref(),
            &impacts,
        );
        let outcome = if candidate_error.is_some() {
            LibrarySkillUpdateCheckOutcome::InvalidCandidate
        } else if upstream_hash.is_none() {
            LibrarySkillUpdateCheckOutcome::NotUpdatable
        } else if upstream_hash.as_deref() == current_hash.as_deref()
            && upstream_hash.as_deref() == Some(skill.content_hash.as_str())
        {
            LibrarySkillUpdateCheckOutcome::UpToDate
        } else {
            LibrarySkillUpdateCheckOutcome::UpdateAvailable
        };
        let local_modified = current_hash.as_deref() != Some(skill.content_hash.as_str());
        Ok(LibrarySkillUpdateCheck {
            library_skill_id: skill.id,
            outcome,
            observation_token,
            stage_token: None,
            recorded_content_hash: skill.content_hash,
            live_content_hash: current_hash,
            staged_content_hash: upstream_hash,
            local_modified,
            compatibility: staged_compatibility,
            affected_deployments: impacts,
            message: candidate_error,
        })
    }

    /// Alias used by command implementations; the snapshot variant remains
    /// the deterministic public fixture seam.
    pub fn check_from_repository_snapshot(
        db: &Arc<Database>,
        library_skill_id: &str,
        repository_root: &Path,
    ) -> Result<LibrarySkillUpdateCheck> {
        Self::inspect_from_repository_snapshot(db, library_skill_id, repository_root)
    }

    /// Return a structured no-upstream result for local/ZIP snapshots without
    /// touching the filesystem or creating a stage directory.
    pub fn check_not_updatable(
        db: &Arc<Database>,
        library_skill_id: &str,
    ) -> Result<LibrarySkillUpdateCheck> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let skill = db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let live_content_hash = Self::live_hash(&skill)?;
        let impacts = Self::deployment_impacts(db, &skill, None)?;
        let observation_token =
            Self::observation_token(&skill, live_content_hash.as_deref(), None, &impacts);
        let local_modified = live_content_hash.as_deref() != Some(skill.content_hash.as_str());
        Ok(LibrarySkillUpdateCheck {
            library_skill_id: skill.id,
            outcome: LibrarySkillUpdateCheckOutcome::NotUpdatable,
            observation_token,
            stage_token: None,
            recorded_content_hash: skill.content_hash,
            live_content_hash,
            staged_content_hash: None,
            local_modified,
            compatibility: None,
            affected_deployments: impacts,
            message: Some("Library Skill source has no upstream update origin".to_string()),
        })
    }

    /// Stage a checked-out upstream snapshot under a managed, non-Library
    /// directory.  No Library row or deployment is changed by this operation.
    pub fn stage_from_repository_snapshot(
        db: &Arc<Database>,
        library_skill_id: &str,
        repository_root: &Path,
    ) -> Result<LibrarySkillUpdateCheck> {
        let mut inspection =
            Self::inspect_from_repository_snapshot(db, library_skill_id, repository_root)?;
        if matches!(
            inspection.outcome,
            LibrarySkillUpdateCheckOutcome::NotUpdatable
                | LibrarySkillUpdateCheckOutcome::InvalidCandidate
        ) {
            return Ok(inspection);
        }
        let upstream_hash = inspection
            .staged_content_hash
            .clone()
            .ok_or_else(|| anyhow!("upstream snapshot has no content hash"))?;
        let compatibility = inspection
            .compatibility
            .clone()
            .ok_or_else(|| anyhow!("upstream snapshot has no compatibility metadata"))?;
        if inspection.outcome == LibrarySkillUpdateCheckOutcome::UpToDate {
            return Ok(inspection);
        }

        let skill = db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let source = Self::snapshot_path(&skill, repository_root)?;
        let token = uuid::Uuid::new_v4().to_string();
        let stage_root = Self::stage_root()?.join(&token);
        let stage = stage_root.join("skill");
        if let Err(error) =
            LibrarySkillAcquisitionService::copy_tree_preserving_links(&source, &stage)
        {
            let _ = fs::remove_dir_all(&stage_root);
            return Err(error);
        }
        let staged_hash = match LibrarySkillAcquisitionService::compute_library_hash(&stage) {
            Ok(hash) => hash,
            Err(error) => {
                let cleanup = fs::remove_dir_all(&stage_root).err();
                return Err(match cleanup {
                    Some(cleanup) => anyhow!(
                        "staged upstream hash failed ({error}); stage cleanup failed: {cleanup}"
                    ),
                    None => error,
                });
            }
        };
        if staged_hash != upstream_hash {
            let _ = fs::remove_dir_all(&stage_root);
            return Err(anyhow!("upstream snapshot changed while staging"));
        }
        let manifest = StageManifest {
            library_skill_id: library_skill_id.to_string(),
            observation_token: inspection.observation_token.clone(),
            upstream_hash: upstream_hash.clone(),
            compatibility: compatibility.clone(),
            source: None,
            external_observation: None,
            external_candidate_id: None,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        if let Err(error) = fs::write(stage_root.join("manifest.json"), manifest_bytes) {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            return Err(match cleanup {
                Some(cleanup) => {
                    anyhow!(
                        "staged manifest write failed ({error}); stage cleanup failed: {cleanup}"
                    )
                }
                None => error.into(),
            });
        }
        inspection.stage_token = Some(token);
        Ok(inspection)
    }

    /// Stage an explicitly selected external snapshot without changing the Library row.
    pub(crate) fn stage_external(
        db: &Arc<Database>,
        skill: &LibrarySkill,
        path: &Path,
        source: LibrarySkillSource,
        external_root: &Path,
        external_token: &str,
        external_candidate_id: &str,
    ) -> Result<LibrarySkillUpdateCheck> {
        let metadata = LibrarySkillAcquisitionService::inspect_source_directory(path)?;
        let live =
            LibrarySkillAcquisitionService::compute_library_hash(&Self::library_path(skill)?)?;
        let impacts = Self::deployment_impacts(db, skill, Some(&metadata.compatibility))?;
        let observation_token =
            Self::observation_token(skill, Some(&live), Some(&metadata.content_hash), &impacts);
        let token = uuid::Uuid::new_v4().to_string();
        let stage_root = Self::stage_root()?.join(&token);
        let stage = stage_root.join("skill");
        let staged = (|| -> Result<()> {
            LibrarySkillAcquisitionService::copy_tree_preserving_links(path, &stage)?;
            if LibrarySkillAcquisitionService::compute_library_hash(&stage)?
                != metadata.content_hash
            {
                return Err(anyhow!(
                    "External Skill changed while staging; inspect again"
                ));
            }
            fs::write(
                stage_root.join("manifest.json"),
                serde_json::to_vec(&StageManifest {
                    library_skill_id: skill.id.clone(),
                    observation_token: observation_token.clone(),
                    upstream_hash: metadata.content_hash.clone(),
                    compatibility: metadata.compatibility.clone(),
                    source: Some(source),
                    external_observation: Some((
                        external_root.to_path_buf(),
                        external_token.to_string(),
                    )),
                    external_candidate_id: Some(external_candidate_id.to_string()),
                })?,
            )?;
            Ok(())
        })();
        if let Err(error) = staged {
            let _ = fs::remove_dir_all(&stage_root);
            return Err(error);
        }
        Ok(LibrarySkillUpdateCheck {
            library_skill_id: skill.id.clone(),
            outcome: LibrarySkillUpdateCheckOutcome::UpdateAvailable,
            observation_token,
            stage_token: Some(token),
            recorded_content_hash: skill.content_hash.clone(),
            live_content_hash: Some(live.clone()),
            staged_content_hash: Some(metadata.content_hash),
            local_modified: live != skill.content_hash,
            compatibility: Some(metadata.compatibility),
            affected_deployments: impacts,
            message: None,
        })
    }

    pub(crate) fn discard_external_stage(token: &str) -> Result<()> {
        let token =
            Self::validate_stage_token(token).ok_or_else(|| anyhow!("Invalid stage token"))?;
        let path = Self::stage_root()?.join(token);
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }

    /// Apply a previously staged snapshot.  The Library lock is acquired
    /// before the Deployment lock, matching acquisition/import composition.
    pub fn apply(
        db: &Arc<Database>,
        intent: LibrarySkillUpdateApplyIntent,
    ) -> Result<LibrarySkillUpdateResult> {
        let library_skill_id = intent.library_skill_id.clone();
        let result = Self::apply_inner(db, intent);
        Self::record_update_activity(db, &library_skill_id, &result);
        result
    }

    fn apply_inner(
        db: &Arc<Database>,
        intent: LibrarySkillUpdateApplyIntent,
    ) -> Result<LibrarySkillUpdateResult> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let _library_guard = LibrarySkillAcquisitionService::lock_for_composite()?;
        let _deployment_guard = SkillDeploymentService::lock_for_composite()?;

        let Some(skill) = db.get_library_skill_by_id(&intent.library_skill_id)? else {
            return Err(anyhow!(
                "Library Skill not found: {}",
                intent.library_skill_id
            ));
        };
        let token = Self::validate_stage_token(&intent.stage_token)
            .ok_or_else(|| anyhow!("invalid or missing staged update token"))?;
        let stage_root = Self::stage_root()?.join(&token);
        let manifest_path = stage_root.join("manifest.json");
        let stage = stage_root.join("skill");
        let manifest = match fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StageManifest>(&bytes).ok())
        {
            Some(manifest) if manifest.library_skill_id == skill.id => manifest,
            _ => {
                let cleanup = fs::remove_dir_all(&stage_root).err();
                return Ok(Self::update_result(
                    LibrarySkillUpdateApplyOutcome::Blocked,
                    &skill.id,
                    Some(LibrarySkillUpdateReason::MissingStage),
                    Some(match cleanup {
                        Some(error) => format!(
                            "staged Library update is missing or invalid; stage cleanup failed: {error}"
                        ),
                        None => "staged Library update is missing or invalid".to_string(),
                    }),
                    None,
                    None,
                    None,
                    Vec::new(),
                    None,
                ));
            }
        };
        let stage_metadata = match fs::symlink_metadata(&stage) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                match LibrarySkillAcquisitionService::inspect_source_directory(&stage) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        let cleanup = fs::remove_dir_all(&stage_root).err();
                        return Ok(Self::update_result(
                            LibrarySkillUpdateApplyOutcome::Stale,
                            &skill.id,
                            Some(LibrarySkillUpdateReason::InvalidCandidate),
                            Some(match cleanup {
                                Some(cleanup) => format!(
                                    "staged candidate is invalid: {error}; stage cleanup failed: {cleanup}"
                                ),
                                None => format!("staged candidate is invalid: {error}"),
                            }),
                            Some(skill.content_hash.clone()),
                            None,
                            None,
                            Vec::new(),
                            None,
                        ));
                    }
                }
            }
            _ => {
                let cleanup = fs::remove_dir_all(&stage_root).err();
                return Ok(Self::update_result(
                    LibrarySkillUpdateApplyOutcome::Blocked,
                    &skill.id,
                    Some(LibrarySkillUpdateReason::MissingStage),
                    Some(match cleanup {
                        Some(error) => format!(
                            "staged Library update directory is missing; stage cleanup failed: {error}"
                        ),
                        None => "staged Library update directory is missing".to_string(),
                    }),
                    Some(skill.content_hash.clone()),
                    None,
                    None,
                    Vec::new(),
                    None,
                ));
            }
        };
        if stage_metadata.content_hash != manifest.upstream_hash {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Stale,
                &skill.id,
                Some(LibrarySkillUpdateReason::InvalidCandidate),
                Some(match cleanup {
                    Some(cleanup) => {
                        format!("staged candidate hash changed; stage cleanup failed: {cleanup}")
                    }
                    None => "staged candidate hash changed".to_string(),
                }),
                Some(skill.content_hash.clone()),
                None,
                Some(stage_metadata.content_hash),
                Vec::new(),
                None,
            ));
        }
        if manifest.observation_token != intent.observation_token {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Stale,
                &skill.id,
                Some(LibrarySkillUpdateReason::StaleObservation),
                Some(match cleanup {
                    Some(error) => format!(
                        "staged update observation is stale; check again; stage cleanup failed: {error}"
                    ),
                    None => "staged update observation is stale; check again".to_string(),
                }),
                Some(skill.content_hash.clone()),
                None,
                Some(manifest.upstream_hash),
                Vec::new(),
                None,
            ));
        }

        if let Some((root, expected)) = &manifest.external_observation {
            let fresh = super::external_skills::ExternalSkillService::inspect_for_observation(
                db,
                root,
                manifest.external_candidate_id.as_deref(),
                &skill.id,
                expected,
            )?;
            if !fresh.accepts_target_observation(expected, &skill.id) {
                let _ = fs::remove_dir_all(&stage_root);
                return Err(anyhow!("External Skill observation changed; inspect again"));
            }
        }
        let destination = Self::library_path(&skill)?;
        let live_hash = match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Some(
                LibrarySkillAcquisitionService::compute_library_hash(&destination)?,
            ),
            _ => None,
        };
        let impacts = Self::deployment_impacts(db, &skill, Some(&stage_metadata.compatibility))?;
        let fresh_token = Self::observation_token(
            &skill,
            live_hash.as_deref(),
            Some(manifest.upstream_hash.as_str()),
            &impacts,
        );
        if fresh_token != intent.observation_token {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Stale,
                &skill.id,
                Some(LibrarySkillUpdateReason::StaleObservation),
                Some(match cleanup {
                    Some(error) => format!(
                        "Library content or desired Deployment state changed; stage cleanup failed: {error}"
                    ),
                    None => "Library content or desired Deployment state changed".to_string(),
                }),
                Some(skill.content_hash.clone()),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if live_hash.is_none() {
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Blocked,
                &skill.id,
                Some(LibrarySkillUpdateReason::StaleObservation),
                Some("Library snapshot is missing or not a real directory".to_string()),
                Some(skill.content_hash.clone()),
                None,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if live_hash.as_deref() == Some(manifest.upstream_hash.as_str())
            && live_hash.as_deref() == Some(skill.content_hash.as_str())
            && manifest
                .source
                .as_ref()
                .map(|source| source == &skill.source)
                .unwrap_or(true)
        {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::UpToDate,
                &skill.id,
                None,
                cleanup.map(|error| {
                    format!("Library is already up to date; stage cleanup failed: {error}")
                }),
                Some(skill.content_hash),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if live_hash.as_deref() != Some(skill.content_hash.as_str())
            && !intent.confirm_local_modifications
        {
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Blocked,
                &skill.id,
                Some(LibrarySkillUpdateReason::LocalModificationConfirmationRequired),
                Some(
                    "Library snapshot has local modifications; explicit confirmation is required"
                        .to_string(),
                ),
                Some(skill.content_hash.clone()),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if Self::has_compatibility_regression(impacts.iter().map(|impact| impact.staged_compatible))
        {
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Blocked,
                &skill.id,
                Some(LibrarySkillUpdateReason::CompatibilityRegression),
                Some("staged snapshot is incompatible with a desired Deployment".to_string()),
                Some(skill.content_hash.clone()),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if db
            .get_library_skill_by_content_hash(&manifest.upstream_hash)?
            .is_some_and(|other| other.id != skill.id)
        {
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Blocked,
                &skill.id,
                Some(LibrarySkillUpdateReason::DuplicateContent),
                Some("staged snapshot is already admitted under another Library Skill".to_string()),
                Some(skill.content_hash.clone()),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }

        let metadata = stage_metadata;
        let backup_root = crate::services::skill_import::create_backup_root()?;
        let backup_snapshot = backup_root.join("library-old");
        if let Err(error) = LibrarySkillAcquisitionService::copy_tree_preserving_links(
            &destination,
            &backup_snapshot,
        ) {
            let _ = fs::remove_dir_all(&stage_root);
            let _ = fs::remove_dir_all(&backup_root);
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::RolledBack,
                &skill.id,
                None,
                Some(format!("failed to create managed Library backup: {error}")),
                Some(skill.content_hash),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        if LibrarySkillAcquisitionService::compute_library_hash(&backup_snapshot).ok() != live_hash
        {
            let _ = fs::remove_dir_all(&stage_root);
            let _ = fs::remove_dir_all(&backup_root);
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::RolledBack,
                &skill.id,
                None,
                Some("managed Library backup hash did not match live snapshot".to_string()),
                Some(skill.content_hash),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }
        // Close the backup-copy TOCTOU window: both trees must still match the
        // hashes observed before creating the managed backup immediately
        // before the atomic exchange.
        let live_before_swap =
            LibrarySkillAcquisitionService::compute_library_hash(&destination).ok();
        let staged_before_swap = LibrarySkillAcquisitionService::compute_library_hash(&stage).ok();
        if live_before_swap != live_hash
            || staged_before_swap.as_deref() != Some(manifest.upstream_hash.as_str())
        {
            let _ = fs::remove_dir_all(&stage_root);
            let _ = fs::remove_dir_all(&backup_root);
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::Stale,
                &skill.id,
                Some(LibrarySkillUpdateReason::StaleObservation),
                Some(
                    "Library or staged content changed immediately before atomic swap".to_string(),
                ),
                Some(skill.content_hash),
                live_before_swap,
                staged_before_swap,
                impacts,
                None,
            ));
        }
        if let Err(error) = Self::atomic_swap_dirs(&destination, &stage) {
            let cleanup = fs::remove_dir_all(&stage_root).err();
            let _ = fs::remove_dir_all(&backup_root);
            if let Some(cleanup) = cleanup {
                return Ok(Self::update_result(
                    LibrarySkillUpdateApplyOutcome::RolledBack,
                    &skill.id,
                    None,
                    Some(format!("atomic Library snapshot swap failed ({error}); stage cleanup failed: {cleanup}")),
                    Some(skill.content_hash),
                    live_hash,
                    Some(manifest.upstream_hash),
                    impacts,
                    None,
                ));
            }
            return Ok(Self::update_result(
                LibrarySkillUpdateApplyOutcome::RolledBack,
                &skill.id,
                None,
                Some(format!("atomic Library snapshot swap failed: {error}")),
                Some(skill.content_hash),
                live_hash,
                Some(manifest.upstream_hash),
                impacts,
                None,
            ));
        }

        let mut replacement = skill.clone();
        if let Some(source) = manifest.source {
            replacement.source = source;
        }
        replacement.display_name = metadata.display_name;
        replacement.description = metadata.description;
        replacement.compatibility = metadata.compatibility;
        replacement.content_hash = metadata.content_hash.clone();
        replacement.updated_at = Utc::now().timestamp();
        let db_result = db.update_library_skill_snapshot(&replacement);
        match db_result {
            Ok(Some(_)) => {
                let cleanup = fs::remove_dir_all(&stage_root).err();
                if let Some(cleanup) = cleanup {
                    return Ok(Self::update_result(
                        LibrarySkillUpdateApplyOutcome::Updated,
                        &skill.id,
                        None,
                        Some(format!("Library updated; stage cleanup failed: {cleanup}")),
                        Some(skill.content_hash),
                        Some(metadata.content_hash.clone()),
                        Some(metadata.content_hash),
                        impacts,
                        Some(backup_root.to_string_lossy().to_string()),
                    ));
                }
                Ok(Self::update_result(
                    LibrarySkillUpdateApplyOutcome::Updated,
                    &skill.id,
                    None,
                    None,
                    Some(skill.content_hash),
                    Some(replacement.content_hash.clone()),
                    Some(replacement.content_hash),
                    impacts,
                    Some(backup_root.to_string_lossy().to_string()),
                ))
            }
            Ok(None) | Err(_) => {
                let row_disappeared = matches!(&db_result, Ok(None));
                let primary = match db_result {
                    Ok(None) => "Library Skill disappeared while applying update".to_string(),
                    Err(error) => format!("Library metadata update failed: {error}"),
                    Ok(Some(_)) => unreachable!(),
                };
                let mut compensation = Vec::new();
                if let Err(error) = Self::atomic_swap_dirs(&destination, &stage) {
                    compensation.push(format!("restore previous Library snapshot failed: {error}"));
                }
                if row_disappeared {
                    match db.get_library_skill_by_id(&skill.id) {
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            if let Err(error) = db.save_library_skill(&skill) {
                                compensation.push(format!(
                                    "restore disappeared Library row failed: {error}"
                                ));
                            }
                        }
                        Err(error) => compensation
                            .push(format!("verify disappeared Library row failed: {error}")),
                    }
                }
                if compensation.is_empty() {
                    let cleanup = fs::remove_dir_all(&stage_root).err();
                    if cleanup.is_none() {
                        let _ = fs::remove_dir_all(&backup_root);
                    }
                    Ok(Self::update_result(
                        LibrarySkillUpdateApplyOutcome::RolledBack,
                        &skill.id,
                        None,
                        cleanup.map(|error| format!("{primary}; stage cleanup failed: {error}")),
                        Some(skill.content_hash),
                        live_hash,
                        Some(replacement.content_hash),
                        impacts,
                        None,
                    ))
                } else {
                    Ok(Self::update_result(
                        LibrarySkillUpdateApplyOutcome::RecoveryRequired,
                        &skill.id,
                        Some(LibrarySkillUpdateReason::CompensationFailed),
                        Some(format!(
                            "{primary}; compensation failed: {}",
                            compensation.join("; ")
                        )),
                        Some(skill.content_hash),
                        live_hash,
                        Some(replacement.content_hash),
                        impacts,
                        Some(backup_root.to_string_lossy().to_string()),
                    ))
                }
            }
        }
    }

    /// Read-only deletion plan.  Only an exact, reachable expected link is
    /// eligible for automatic removal; every drifted or inaccessible target
    /// requires the caller to use the existing explicit Deployment Forget
    /// intent first.
    pub fn inspect_deletion(
        db: &Arc<Database>,
        library_skill_id: &str,
    ) -> Result<LibrarySkillDeletionInspection> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let skill = db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let (live_hash, snapshot_missing, library_block_message) =
            match Self::deletion_snapshot(&skill) {
                Ok(LibraryDeletionSnapshot::Present(hash)) => (Some(hash), false, None),
                Ok(LibraryDeletionSnapshot::Missing) => (None, true, None),
                Ok(LibraryDeletionSnapshot::Unsafe) => (
                    None,
                    false,
                    Some("Library snapshot is not a real directory".to_string()),
                ),
                Err(error) => (
                    None,
                    false,
                    Some(format!("Library snapshot is unreadable: {error}")),
                ),
            };
        let deployment_service = SkillDeploymentService::new(db.clone());
        let mut targets = Vec::new();
        for desired in db
            .list_skill_deployments()?
            .into_iter()
            .filter(|deployment| deployment.library_skill_id == library_skill_id)
        {
            let inspection = deployment_service
                .inspect(DeploymentQuery::for_target(desired.target.clone()))?
                .items
                .into_iter()
                .find(|item| item.library_skill_id == library_skill_id)
                .ok_or_else(|| {
                    anyhow!("Deployment inspection item not found: {library_skill_id}")
                })?;
            let exact_expected_link = inspection.desired.is_some()
                && (inspection.observed.state
                    == crate::services::skill_deployment::ObservedDeploymentState::CorrectLink
                    || (snapshot_missing
                        && deployment_service
                            .expected_link_is_removable_for_composite(&desired)
                            .unwrap_or(false)));
            let action_required = if exact_expected_link {
                LibrarySkillDeletionAction::RemoveExpectedLink
            } else {
                LibrarySkillDeletionAction::Forget
            };
            targets.push(LibrarySkillDeletionTarget {
                inspection,
                action_required,
            });
        }
        let blocked_by_target = targets
            .iter()
            .any(|target| target.action_required == LibrarySkillDeletionAction::Forget);
        let blocked = library_block_message.is_some() || blocked_by_target;
        let observation_token =
            Self::deletion_observation_token(&skill, live_hash.as_deref(), &targets);
        Ok(LibrarySkillDeletionInspection {
            library_skill_id: library_skill_id.to_string(),
            observation_token,
            targets,
            blocked,
            message: if let Some(message) = library_block_message {
                Some(message)
            } else if blocked_by_target {
                Some(
                    "drifted or inaccessible Deployment targets require explicit Forget before Library deletion"
                        .to_string(),
                )
            } else {
                None
            },
        })
    }

    /// Remove all exact links and then delete the Library row/snapshot.  The
    /// operation is serialized as Library → Deployment and compensates links
    /// in reverse order when a later filesystem or DB step fails.
    pub fn delete(
        db: &Arc<Database>,
        intent: LibrarySkillDeletionIntent,
    ) -> Result<LibrarySkillDeletionResult> {
        let library_skill_id = intent.library_skill_id.clone();
        let result = Self::delete_inner(db, intent);
        Self::record_deletion_activity(db, &library_skill_id, &result);
        result
    }

    fn delete_inner(
        db: &Arc<Database>,
        intent: LibrarySkillDeletionIntent,
    ) -> Result<LibrarySkillDeletionResult> {
        LibrarySkillAcquisitionService::ensure_supported_platform()?;
        let _library_guard = LibrarySkillAcquisitionService::lock_for_composite()?;
        let _deployment_guard = SkillDeploymentService::lock_for_composite()?;
        let Some(skill) = db.get_library_skill_by_id(&intent.library_skill_id)? else {
            return Err(anyhow!(
                "Library Skill not found: {}",
                intent.library_skill_id
            ));
        };
        let plan = Self::inspect_deletion(db, &intent.library_skill_id)?;
        if plan.observation_token != intent.observation_token {
            return Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::Stale,
                library_skill_id: skill.id,
                items: Vec::new(),
                backup_path: None,
                message: Some("deletion observation is stale; inspect again".to_string()),
            });
        }
        let observed_live_hash = match Self::deletion_snapshot(&skill) {
            Ok(LibraryDeletionSnapshot::Present(hash)) => Some(hash),
            Ok(LibraryDeletionSnapshot::Missing) => None,
            Ok(LibraryDeletionSnapshot::Unsafe) => {
                return Ok(LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::Blocked,
                    library_skill_id: skill.id,
                    items: Vec::new(),
                    backup_path: None,
                    message: Some("Library snapshot is not a real directory".to_string()),
                });
            }
            Err(error) => {
                return Ok(LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::Blocked,
                    library_skill_id: skill.id,
                    items: Vec::new(),
                    backup_path: None,
                    message: Some(format!("Library snapshot became unreadable: {error}")),
                });
            }
        };
        let fresh_deletion_token =
            Self::deletion_observation_token(&skill, observed_live_hash.as_deref(), &plan.targets);
        if fresh_deletion_token != intent.observation_token {
            return Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::Stale,
                library_skill_id: skill.id,
                items: Vec::new(),
                backup_path: None,
                message: Some("Library snapshot changed; inspect deletion again".to_string()),
            });
        }
        let deployment_service = SkillDeploymentService::new(db.clone());
        let mut items = Vec::new();
        let mut removed: Vec<DesiredDeployment> = Vec::new();
        for target in plan.targets.iter().filter(|target| {
            target.action_required == LibrarySkillDeletionAction::RemoveExpectedLink
        }) {
            let result = match target
                .inspection
                .desired
                .as_ref()
                .ok_or_else(|| anyhow!("deletion target has no desired Deployment"))
                .and_then(|desired| deployment_service.remove_expected_link_for_composite(desired))
            {
                Ok(result) => result,
                Err(error) => Self::error_deletion_item(&target.inspection, error.to_string()),
            };
            let successful = matches!(
                result.outcome,
                DeploymentMutationOutcome::Removed | DeploymentMutationOutcome::AlreadyAbsent
            );
            if successful {
                if let Some(desired) = target.inspection.desired.clone() {
                    removed.push(desired);
                }
            } else if result.outcome == DeploymentMutationOutcome::Error {
                // An Error may mean DB deletion happened after filesystem
                // removal but its compensation failed.  Include this current
                // row in the reverse-order restore audit, not only prior
                // successful removals.
                if let Some(desired) = target.inspection.desired.clone() {
                    removed.push(desired);
                }
            }
            items.push(result);
            if !successful {
                let compensation = Self::restore_deployments(&deployment_service, &removed);
                if compensation.is_empty() {
                    return Ok(LibrarySkillDeletionResult {
                        outcome: LibrarySkillDeletionOutcome::RolledBack,
                        library_skill_id: skill.id,
                        items,
                        backup_path: None,
                        message: Some(
                            "Deployment removal failed; previously removed targets were restored"
                                .to_string(),
                        ),
                    });
                }
                return Ok(LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(format!(
                        "Deployment removal failed; compensation failed: {}",
                        compensation.join("; ")
                    )),
                });
            }
        }

        // Safe links are removed even when another target is drifted.  The
        // remaining desired rows stay as explicit Forget blockers, so the
        // Library snapshot and metadata are retained for a later retry.
        if plan.blocked {
            items.extend(
                plan.targets
                    .iter()
                    .filter(|target| target.action_required == LibrarySkillDeletionAction::Forget)
                    .map(|target| {
                        Self::blocked_deletion_item(
                            target,
                            "explicit Deployment Forget is required before deletion",
                        )
                    }),
            );
            return Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::Blocked,
                library_skill_id: skill.id,
                items,
                backup_path: None,
                message: plan.message.clone().or_else(|| {
                    Some(
                        "drifted or inaccessible Deployment targets require explicit Forget"
                            .to_string(),
                    )
                }),
            });
        }

        if observed_live_hash.is_none() {
            let deletion_result = db.delete_library_skill(&skill.id);
            if matches!(&deletion_result, Ok(true)) {
                return Ok(LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::Deleted,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: None,
                });
            }
            let primary = match &deletion_result {
                Ok(false) => "Library Skill disappeared during deletion".to_string(),
                Err(error) => format!("Library row deletion failed: {error}"),
                Ok(true) => unreachable!(),
            };
            let mut compensation = Vec::new();
            if matches!(&deletion_result, Ok(false)) {
                match db.get_library_skill_by_id(&skill.id) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if let Err(error) = db.save_library_skill(&skill) {
                            compensation
                                .push(format!("restore disappeared Library row failed: {error}"));
                        }
                    }
                    Err(error) => {
                        compensation.push(format!("verify disappeared Library row failed: {error}"))
                    }
                }
            }
            compensation.extend(Self::restore_deployments(&deployment_service, &removed));
            return Ok(if compensation.is_empty() {
                LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::RolledBack,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(primary),
                }
            } else {
                LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(format!(
                        "{primary}; compensation failed: {}",
                        compensation.join("; ")
                    )),
                }
            });
        }

        let destination = Self::library_path(&skill)?;
        let backup_root = match crate::services::skill_import::create_backup_root() {
            Ok(path) => path,
            Err(error) => {
                let compensation = Self::restore_deployments(&deployment_service, &removed);
                return Ok(if compensation.is_empty() {
                    LibrarySkillDeletionResult {
                        outcome: LibrarySkillDeletionOutcome::RolledBack,
                        library_skill_id: skill.id,
                        items,
                        backup_path: None,
                        message: Some(format!("Library backup creation failed: {error}")),
                    }
                } else {
                    LibrarySkillDeletionResult {
                        outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                        library_skill_id: skill.id,
                        items,
                        backup_path: None,
                        message: Some(format!(
                            "Library backup creation failed: {error}; compensation failed: {}",
                            compensation.join("; ")
                        )),
                    }
                });
            }
        };
        let deleted_staging = backup_root.join("library-deleted");
        #[cfg(debug_assertions)]
        let forced_rename_failure = FORCE_DELETION_RENAME_FAILURE.swap(false, Ordering::SeqCst);
        #[cfg(not(debug_assertions))]
        let forced_rename_failure = false;
        let rename_result = if forced_rename_failure {
            Err(std::io::Error::other(
                "injected Library deletion rename failure",
            ))
        } else {
            fs::rename(&destination, &deleted_staging)
        };
        if let Err(error) = rename_result {
            let compensation = Self::restore_deployments(&deployment_service, &removed);
            return Ok(if compensation.is_empty() {
                let _ = fs::remove_dir_all(&backup_root);
                LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::RolledBack,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(format!("Library snapshot removal failed: {error}")),
                }
            } else {
                let cleanup = fs::remove_dir_all(&backup_root).err();
                LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(format!(
                        "Library source remained in place; Deployment recovery is required after snapshot removal failed: {}{}",
                        compensation.join("; "),
                        cleanup
                            .map(|cleanup| format!("; empty backup cleanup failed: {cleanup}"))
                            .unwrap_or_default(),
                    )),
                }
            });
        }
        let deleted_hash =
            LibrarySkillAcquisitionService::compute_library_hash(&deleted_staging).ok();
        let observed_live_hash = observed_live_hash.expect("present snapshot has a live hash");
        if deleted_hash.as_deref() != Some(observed_live_hash.as_str()) {
            let mut compensation = Vec::new();
            let source_restored = match fs::rename(&deleted_staging, &destination) {
                Ok(()) => true,
                Err(error) => {
                    compensation.push(format!("Library snapshot stale restore failed: {error}"));
                    false
                }
            };
            compensation.extend(Self::restore_deployments(&deployment_service, &removed));
            if compensation.is_empty() {
                let _ = fs::remove_dir_all(&backup_root);
                return Ok(LibrarySkillDeletionResult {
                    outcome: LibrarySkillDeletionOutcome::Stale,
                    library_skill_id: skill.id,
                    items,
                    backup_path: None,
                    message: Some(
                        "Library snapshot changed before deletion; inspect again".to_string(),
                    ),
                });
            }
            let recoverable_backup = !source_restored && deleted_staging.exists();
            let backup_path = recoverable_backup.then(|| backup_root.to_string_lossy().to_string());
            if !recoverable_backup {
                let _ = fs::remove_dir_all(&backup_root);
            }
            return Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                library_skill_id: skill.id,
                items,
                backup_path,
                message: Some(format!(
                    "Library snapshot changed before deletion; compensation failed{}: {}",
                    if recoverable_backup {
                        "; a recovery snapshot was retained"
                    } else {
                        ""
                    },
                    compensation.join("; ")
                )),
            });
        }
        let deletion_result = db.delete_library_skill(&skill.id);
        if matches!(&deletion_result, Ok(true)) {
            return Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::Deleted,
                library_skill_id: skill.id,
                items,
                backup_path: Some(backup_root.to_string_lossy().to_string()),
                message: None,
            });
        }
        let primary = match &deletion_result {
            Ok(false) => "Library Skill disappeared during deletion".to_string(),
            Err(error) => format!("Library row deletion failed: {error}"),
            Ok(true) => unreachable!(),
        };
        let mut compensation = Vec::new();
        let source_restored = match fs::rename(&deleted_staging, &destination) {
            Ok(()) => true,
            Err(error) => {
                compensation.push(format!("Library snapshot restore failed: {error}"));
                false
            }
        };
        if matches!(&deletion_result, Ok(false)) {
            match db.get_library_skill_by_id(&skill.id) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Err(error) = db.save_library_skill(&skill) {
                        compensation
                            .push(format!("restore disappeared Library row failed: {error}"));
                    }
                }
                Err(error) => {
                    compensation.push(format!("verify disappeared Library row failed: {error}"))
                }
            }
        }
        compensation.extend(Self::restore_deployments(&deployment_service, &removed));
        if compensation.is_empty() {
            let _ = fs::remove_dir_all(&backup_root);
            Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::RolledBack,
                library_skill_id: skill.id,
                items,
                backup_path: None,
                message: Some(primary),
            })
        } else {
            let recoverable_backup = !source_restored
                && deleted_staging.is_dir()
                && LibrarySkillAcquisitionService::compute_library_hash(&deleted_staging).ok()
                    == Some(observed_live_hash.clone());
            let backup_path = recoverable_backup.then(|| backup_root.to_string_lossy().to_string());
            if !recoverable_backup {
                let _ = fs::remove_dir_all(&backup_root);
            }
            let recovery_message = if source_restored {
                format!(
                    "{primary}; Library source remains in place; Deployment recovery required: {}",
                    compensation.join("; ")
                )
            } else if recoverable_backup {
                format!(
                    "{primary}; compensation failed: {}",
                    compensation.join("; ")
                )
            } else {
                format!(
                    "{primary}; Library snapshot restore failed and no verified backup remains; Deployment recovery required: {}",
                    compensation.join("; ")
                )
            };
            Ok(LibrarySkillDeletionResult {
                outcome: LibrarySkillDeletionOutcome::RecoveryRequired,
                library_skill_id: skill.id,
                items,
                backup_path,
                message: Some(recovery_message),
            })
        }
    }

    fn record_update_activity(
        db: &Arc<Database>,
        library_skill_id: &str,
        result: &Result<LibrarySkillUpdateResult>,
    ) {
        let (outcome, detail_code) = match result {
            Ok(result) => match result.outcome {
                LibrarySkillUpdateApplyOutcome::Updated => {
                    (ActivityOutcome::Success, ActivityDetailCode::None)
                }
                LibrarySkillUpdateApplyOutcome::UpToDate => {
                    (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyInSync)
                }
                LibrarySkillUpdateApplyOutcome::Blocked => (
                    ActivityOutcome::Blocked,
                    Self::update_reason_detail(result.reason.as_ref()),
                ),
                LibrarySkillUpdateApplyOutcome::Stale => (
                    ActivityOutcome::Blocked,
                    ActivityDetailCode::StaleObservation,
                ),
                LibrarySkillUpdateApplyOutcome::RolledBack => (
                    ActivityOutcome::RolledBack,
                    ActivityDetailCode::FilesystemFailure,
                ),
                LibrarySkillUpdateApplyOutcome::RecoveryRequired => (
                    ActivityOutcome::CompensationFailed,
                    ActivityDetailCode::CompensationFailure,
                ),
            },
            Err(error) => (
                ActivityOutcome::Failed,
                Self::activity_detail_for_error(error),
            ),
        };
        Self::record_activity(
            db,
            ActivityOperation::Library,
            ActivityReason::Update,
            outcome,
            detail_code,
            library_skill_id,
            None,
            None,
            None,
        );
    }

    fn record_deletion_activity(
        db: &Arc<Database>,
        library_skill_id: &str,
        result: &Result<LibrarySkillDeletionResult>,
    ) {
        if let Ok(result) = result {
            let item_count = result.items.len() as u32;
            let batch_id = (item_count > 0).then(|| uuid::Uuid::new_v4().to_string());
            for (index, item) in result.items.iter().enumerate() {
                let (outcome, detail_code) = match item.outcome {
                    DeploymentMutationOutcome::Removed => {
                        (ActivityOutcome::Success, ActivityDetailCode::None)
                    }
                    DeploymentMutationOutcome::AlreadyAbsent => {
                        (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyAbsent)
                    }
                    DeploymentMutationOutcome::Blocked => (
                        ActivityOutcome::Blocked,
                        ActivityDetailCode::ValidationFailure,
                    ),
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
                    DeploymentMutationOutcome::RecoveryRequired => (
                        ActivityOutcome::CompensationFailed,
                        ActivityDetailCode::CompensationFailure,
                    ),
                    DeploymentMutationOutcome::Error => (
                        ActivityOutcome::Failed,
                        ActivityDetailCode::FilesystemFailure,
                    ),
                    DeploymentMutationOutcome::Applied
                    | DeploymentMutationOutcome::Replaced
                    | DeploymentMutationOutcome::Forgotten => {
                        (ActivityOutcome::Success, ActivityDetailCode::None)
                    }
                    DeploymentMutationOutcome::AlreadyInSync => {
                        (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyInSync)
                    }
                };
                Self::record_activity(
                    db,
                    ActivityOperation::Removal,
                    ActivityReason::DeploymentRemove,
                    outcome,
                    detail_code,
                    library_skill_id,
                    Some(&item.target),
                    item.inspection
                        .as_ref()
                        .and_then(|inspection| inspection.desired.as_ref())
                        .map(|desired| desired.id.clone()),
                    batch_id.as_ref().map(|batch_id| ActivityBatchContext {
                        batch_id: batch_id.clone(),
                        item_index: index as u32,
                        item_count,
                    }),
                );
            }
        }
        let (outcome, detail_code) = match result {
            Ok(result) => match result.outcome {
                LibrarySkillDeletionOutcome::Deleted => {
                    (ActivityOutcome::Success, ActivityDetailCode::None)
                }
                LibrarySkillDeletionOutcome::Blocked => {
                    (ActivityOutcome::Blocked, ActivityDetailCode::Drift)
                }
                LibrarySkillDeletionOutcome::Stale => (
                    ActivityOutcome::Blocked,
                    ActivityDetailCode::StaleObservation,
                ),
                LibrarySkillDeletionOutcome::RolledBack => (
                    ActivityOutcome::RolledBack,
                    ActivityDetailCode::FilesystemFailure,
                ),
                LibrarySkillDeletionOutcome::RecoveryRequired => (
                    ActivityOutcome::CompensationFailed,
                    ActivityDetailCode::CompensationFailure,
                ),
            },
            Err(error) => (
                ActivityOutcome::Failed,
                Self::activity_detail_for_error(error),
            ),
        };
        Self::record_activity(
            db,
            ActivityOperation::Removal,
            ActivityReason::LibraryRemove,
            outcome,
            detail_code,
            library_skill_id,
            None,
            None,
            None,
        );
    }

    fn update_reason_detail(reason: Option<&LibrarySkillUpdateReason>) -> ActivityDetailCode {
        match reason {
            Some(LibrarySkillUpdateReason::StaleObservation) => {
                ActivityDetailCode::StaleObservation
            }
            Some(LibrarySkillUpdateReason::DuplicateContent) => ActivityDetailCode::DuplicateKey,
            Some(LibrarySkillUpdateReason::MissingStage) => ActivityDetailCode::InvalidInput,
            Some(LibrarySkillUpdateReason::CompensationFailed) => {
                ActivityDetailCode::CompensationFailure
            }
            _ => ActivityDetailCode::ValidationFailure,
        }
    }

    fn activity_detail_for_error(error: &anyhow::Error) -> ActivityDetailCode {
        for source in error.chain() {
            if let Some(error) = source.downcast_ref::<AppError>() {
                return match error {
                    AppError::Database(_) => ActivityDetailCode::DatabaseFailure,
                    AppError::InvalidInput(_) | AppError::Config(_) => {
                        ActivityDetailCode::InvalidInput
                    }
                    AppError::Io { .. } | AppError::IoContext { .. } => {
                        ActivityDetailCode::FilesystemFailure
                    }
                    _ => ActivityDetailCode::ValidationFailure,
                };
            }
            if source.downcast_ref::<std::io::Error>().is_some() {
                return ActivityDetailCode::FilesystemFailure;
            }
        }
        ActivityDetailCode::ValidationFailure
    }

    #[allow(clippy::too_many_arguments)]
    fn record_activity(
        db: &Arc<Database>,
        operation: ActivityOperation,
        reason: ActivityReason,
        outcome: ActivityOutcome,
        detail_code: ActivityDetailCode,
        library_skill_id: &str,
        deployment_target: Option<&DeploymentTarget>,
        deployment_id: Option<String>,
        batch: Option<ActivityBatchContext>,
    ) {
        ActivityRecorder::new(db.clone()).record_best_effort(ActivityEventInput {
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
                workspace_id: deployment_target
                    .filter(|target| !target.workspace_id.is_empty())
                    .map(|target| target.workspace_id.clone()),
                deployment_id,
                consumer: deployment_target.map(|target| target.consumer),
                workspace_kind: deployment_target.map(|target| target.workspace),
            },
            batch,
            detail_code,
        });
    }

    fn deletion_observation_token(
        skill: &LibrarySkill,
        live_hash: Option<&str>,
        targets: &[LibrarySkillDeletionTarget],
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&skill.source).unwrap_or_default());
        hasher.update([0]);
        hasher.update(skill.id.as_bytes());
        hasher.update([0]);
        hasher.update(skill.directory.as_bytes());
        hasher.update([0]);
        hasher.update(skill.content_hash.as_bytes());
        hasher.update([0]);
        hasher.update(live_hash.unwrap_or_default().as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(targets).unwrap_or_default());
        format!("{:x}", hasher.finalize())
    }

    fn blocked_deletion_item(
        target: &LibrarySkillDeletionTarget,
        message: &str,
    ) -> DeploymentItemResult {
        DeploymentItemResult {
            library_skill_id: target.inspection.library_skill_id.clone(),
            target: target.inspection.target.clone(),
            outcome: DeploymentMutationOutcome::Blocked,
            message: Some(message.to_string()),
            inspection: Some(target.inspection.clone()),
        }
    }

    fn error_deletion_item(target: &DeploymentInspection, message: String) -> DeploymentItemResult {
        DeploymentItemResult {
            library_skill_id: target.library_skill_id.clone(),
            target: target.target.clone(),
            outcome: DeploymentMutationOutcome::Error,
            message: Some(message),
            inspection: Some(target.clone()),
        }
    }

    fn restore_deployments(
        service: &SkillDeploymentService,
        targets: &[DesiredDeployment],
    ) -> Vec<String> {
        targets
            .iter()
            .rev()
            .filter_map(
                |target| match service.restore_removed_for_composite(target) {
                    Ok(result)
                        if matches!(
                            result.outcome,
                            DeploymentMutationOutcome::Applied
                                | DeploymentMutationOutcome::AlreadyInSync
                        ) =>
                    {
                        None
                    }
                    Ok(result) => Some(format!(
                        "restore {:?} returned {:?}: {}",
                        target.target,
                        result.outcome,
                        result.message.unwrap_or_default()
                    )),
                    Err(error) => Some(format!("restore {:?} failed: {error}", target.target)),
                },
            )
            .collect()
    }

    fn validate_stage_token(raw: &str) -> Option<String> {
        let parsed = uuid::Uuid::parse_str(raw).ok()?;
        let canonical = parsed.to_string();
        (canonical == raw).then_some(canonical)
    }

    #[cfg(target_os = "macos")]
    fn atomic_swap_dirs(left: &Path, right: &Path) -> std::io::Result<()> {
        #[cfg(debug_assertions)]
        if FORCE_ATOMIC_SWAP_FAILURE.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::other(
                "injected atomic Library swap failure",
            ));
        }
        use std::ffi::CString;
        let left = CString::new(left.as_os_str().as_encoded_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
        let right = CString::new(right.as_os_str().as_encoded_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
        let result = unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn update_result(
        outcome: LibrarySkillUpdateApplyOutcome,
        library_skill_id: &str,
        reason: Option<LibrarySkillUpdateReason>,
        message: Option<String>,
        recorded_content_hash: Option<String>,
        live_content_hash: Option<String>,
        staged_content_hash: Option<String>,
        affected_deployments: Vec<UpdateDeploymentImpact>,
        backup_path: Option<String>,
    ) -> LibrarySkillUpdateResult {
        LibrarySkillUpdateResult {
            outcome,
            library_skill_id: library_skill_id.to_string(),
            reason,
            recorded_content_hash,
            live_content_hash,
            staged_content_hash,
            affected_deployments,
            backup_path,
            message,
        }
    }

    fn library_path(skill: &LibrarySkill) -> Result<PathBuf> {
        let directory = &skill.directory;
        if directory.is_empty()
            || directory == "."
            || directory == ".."
            || directory.contains('/')
            || directory.contains('\\')
        {
            return Err(anyhow!("invalid Library directory: {directory}"));
        }
        Ok(LibrarySkillAcquisitionService::library_directory_path().join(directory))
    }

    fn live_hash(skill: &LibrarySkill) -> Result<Option<String>> {
        let path = Self::library_path(skill)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(Some(
                LibrarySkillAcquisitionService::compute_library_hash(&path)?,
            )),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn deletion_snapshot(skill: &LibrarySkill) -> Result<LibraryDeletionSnapshot> {
        let path = Self::library_path(skill)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                Ok(LibraryDeletionSnapshot::Present(
                    LibrarySkillAcquisitionService::compute_library_hash(&path)?,
                ))
            }
            Ok(_) => Ok(LibraryDeletionSnapshot::Unsafe),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(LibraryDeletionSnapshot::Missing)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn snapshot_path(skill: &LibrarySkill, repository_root: &Path) -> Result<PathBuf> {
        let root_metadata = fs::symlink_metadata(repository_root).with_context(|| {
            format!(
                "failed to access upstream snapshot {}",
                repository_root.display()
            )
        })?;
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            return Err(anyhow!("upstream snapshot must be a real directory"));
        }
        let canonical_root = repository_root.canonicalize()?;
        let candidate = match skill.source.skill_path.as_deref() {
            Some(path) if path != "." && !path.is_empty() => repository_root.join(path),
            _ => repository_root.to_path_buf(),
        };
        if !candidate.join("SKILL.md").is_file() {
            return Err(anyhow!("upstream snapshot is missing canonical SKILL.md"));
        }
        let canonical = candidate.canonicalize()?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("upstream Skill source escapes repository snapshot"));
        }
        let metadata = fs::symlink_metadata(&candidate)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(anyhow!("upstream Skill source must be a real directory"));
        }
        Ok(candidate)
    }

    fn stage_root() -> Result<PathBuf> {
        let root = crate::config::get_app_config_dir().join("skill-update-stages");
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn observation_token(
        skill: &LibrarySkill,
        current_hash: Option<&str>,
        upstream_hash: Option<&str>,
        blocked: &[UpdateDeploymentImpact],
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&skill.source).unwrap_or_default());
        hasher.update([0]);
        hasher.update(skill.id.as_bytes());
        hasher.update([0]);
        hasher.update(skill.content_hash.as_bytes());
        hasher.update([0]);
        hasher.update(current_hash.unwrap_or_default().as_bytes());
        hasher.update([0]);
        hasher.update(upstream_hash.unwrap_or_default().as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(blocked).unwrap_or_default());
        format!("{:x}", hasher.finalize())
    }

    fn deployment_impacts(
        db: &Arc<Database>,
        skill: &LibrarySkill,
        staged: Option<&LibrarySkillCompatibility>,
    ) -> Result<Vec<UpdateDeploymentImpact>> {
        let deployment_service = SkillDeploymentService::new(db.clone());
        let mut impacts = Vec::new();
        for desired in db
            .list_skill_deployments()?
            .into_iter()
            .filter(|deployment| deployment.library_skill_id == skill.id)
        {
            let inspection = deployment_service
                .inspect(DeploymentQuery::for_target(desired.target.clone()))?
                .items
                .into_iter()
                .find(|item| item.library_skill_id == skill.id)
                .ok_or_else(|| anyhow!("Deployment inspection item not found: {}", skill.id))?;
            let current_compatible =
                Self::consumer_compatible(&skill.compatibility, desired.target.consumer);
            let staged_compatible = staged
                .map(|compatibility| {
                    Self::consumer_compatible(compatibility, desired.target.consumer)
                })
                .unwrap_or(current_compatible);
            impacts.push(UpdateDeploymentImpact {
                inspection,
                current_compatible,
                staged_compatible,
            });
        }
        Ok(impacts)
    }

    fn consumer_compatible(
        compatibility: &LibrarySkillCompatibility,
        consumer: crate::services::skill_deployment::DeploymentConsumer,
    ) -> bool {
        match consumer {
            crate::services::skill_deployment::DeploymentConsumer::Claude => {
                compatibility.claude.compatible
            }
            crate::services::skill_deployment::DeploymentConsumer::Codex => {
                compatibility.codex.compatible
            }
        }
    }

    fn has_compatibility_regression(staged_compatibility: impl IntoIterator<Item = bool>) -> bool {
        staged_compatibility
            .into_iter()
            .any(|compatible| !compatible)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LibrarySkillDeletionOutcome, LibrarySkillDeletionResult, LibrarySkillUpdateService,
    };
    use crate::database::Database;
    use crate::services::activity::{
        ActivityDetailCode, ActivityOperation, ActivityOutcome, ActivityQuery, ActivityReason,
        ActivityRecorder,
    };
    use crate::services::skill_deployment::{
        DeploymentConsumer, DeploymentItemResult, DeploymentMutationOutcome, DeploymentTarget,
        WorkspaceKind,
    };
    use std::sync::Arc;

    #[test]
    fn compatibility_preflight_blocks_when_any_desired_target_regresses() {
        let compatible = [true, true];
        let regressed = [true, false];
        assert!(!LibrarySkillUpdateService::has_compatibility_regression(
            compatible
        ));
        assert!(LibrarySkillUpdateService::has_compatibility_regression(
            regressed
        ));
    }

    #[test]
    fn deletion_children_keep_semantic_no_ops_and_project_target_identity() {
        let db = Arc::new(Database::memory().expect("create activity database"));
        let target = DeploymentTarget {
            consumer: DeploymentConsumer::Codex,
            workspace: WorkspaceKind::Project,
            workspace_id: "workspace-a".to_string(),
        };
        let item = |outcome| DeploymentItemResult {
            library_skill_id: "skill-a".to_string(),
            target: target.clone(),
            outcome,
            message: None,
            inspection: None,
        };
        let result: anyhow::Result<LibrarySkillDeletionResult> = Ok(LibrarySkillDeletionResult {
            outcome: LibrarySkillDeletionOutcome::Blocked,
            library_skill_id: "skill-a".to_string(),
            items: vec![
                item(DeploymentMutationOutcome::AlreadyAbsent),
                item(DeploymentMutationOutcome::AlreadyInSync),
                item(DeploymentMutationOutcome::Forgotten),
            ],
            backup_path: None,
            message: None,
        });

        LibrarySkillUpdateService::record_deletion_activity(&db, "skill-a", &result);

        let page = ActivityRecorder::new(db)
            .list(ActivityQuery {
                operation: Some(ActivityOperation::Removal),
                reason: Some(ActivityReason::DeploymentRemove),
                ..ActivityQuery::default()
            })
            .expect("list deletion child activity");
        assert_eq!(page.entries.len(), 3);
        for entry in &page.entries {
            assert_eq!(entry.target.consumer, Some(DeploymentConsumer::Codex));
            assert_eq!(entry.target.workspace_kind, Some(WorkspaceKind::Project));
            assert_eq!(entry.target.workspace_id.as_deref(), Some("workspace-a"));
        }
        let by_index = |index| {
            page.entries
                .iter()
                .find(|entry| {
                    entry
                        .batch
                        .as_ref()
                        .is_some_and(|batch| batch.item_index == index)
                })
                .expect("batch child by index")
        };
        assert_eq!(by_index(0).outcome, ActivityOutcome::NoOp);
        assert_eq!(by_index(0).detail_code, ActivityDetailCode::AlreadyAbsent);
        assert_eq!(by_index(1).outcome, ActivityOutcome::NoOp);
        assert_eq!(by_index(1).detail_code, ActivityDetailCode::AlreadyInSync);
        assert_eq!(by_index(2).outcome, ActivityOutcome::Success);
        assert_eq!(by_index(2).detail_code, ActivityDetailCode::None);
    }
}
