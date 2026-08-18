//! Journaled execution of the guided macOS Skills migration.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::config::get_home_dir;
use crate::database::{
    Database, SkillsMigrationItemRecord, SkillsMigrationItemUpdate, SkillsMigrationRunRecord,
};
use crate::services::skill::{
    LibrarySkillAcquisitionService, LibrarySkillSource, LibrarySourceKind,
};
use crate::services::skill_deployment::{
    DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome, DeploymentTarget,
    SkillDeploymentService,
};
use crate::services::skills_migration_preview::{
    inspect_locked, SkillsMigrationAction, SkillsMigrationDisposition, SkillsMigrationPageMode,
    SkillsMigrationPlanItem, SkillsMigrationPreflight, SkillsMigrationReason,
    SkillsMigrationStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationIntent {
    pub observation_token: String,
    #[serde(default)]
    pub preserve_unsupported_consumer_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationRestoreIntent {
    pub backup_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationExecutionOutcome {
    Completed,
    StaleObservation,
    Resumable,
    Blocked,
    RecoveryRequired,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationProgress {
    pub completed_items: u32,
    pub total_items: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationItemOutcome {
    Completed,
    AlreadyCompleted,
    Preserved,
    RolledBack,
    Blocked,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationItemResult {
    pub action: crate::services::skills_migration_preview::SkillsMigrationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<DeploymentConsumer>,
    pub outcome: SkillsMigrationItemOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<crate::services::skills_migration_preview::SkillsMigrationReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationBackupReference {
    pub backup_id: String,
    pub created_at: i64,
    pub restore_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationExecutionResult {
    pub outcome: SkillsMigrationExecutionOutcome,
    pub page_mode: crate::services::skills_migration_preview::SkillsMigrationPageMode,
    pub progress: SkillsMigrationProgress,
    pub items: Vec<SkillsMigrationItemResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<SkillsMigrationBackupReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsMigrationBackupManifest {
    version: u8,
    plan_hash: String,
    database_sha256: String,
    content: Vec<SkillsMigrationBackupContent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsMigrationBackupContent {
    ordinal: u32,
    fingerprint: String,
}

static SKILLS_MIGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(debug_assertions)]
static INTERRUPT_AFTER_DATABASE_RESTORE: OnceLock<Mutex<bool>> = OnceLock::new();
#[cfg(debug_assertions)]
static INTERRUPT_AFTER_SOURCE_RETIRE: OnceLock<Mutex<bool>> = OnceLock::new();

pub struct SkillsMigrationExecutionService {
    db: Arc<Database>,
}

impl SkillsMigrationExecutionService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn lock() -> Result<MutexGuard<'static, ()>> {
        SKILLS_MIGRATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub fn start(&self, intent: SkillsMigrationIntent) -> Result<SkillsMigrationExecutionResult> {
        let _migration_guard = Self::lock()?;
        let _library_guard =
            crate::services::skill::LibrarySkillAcquisitionService::lock_for_composite()?;
        let _deployment_guard =
            crate::services::skill_deployment::SkillDeploymentService::lock_for_composite()?;
        let preflight = inspect_locked(self.db.clone())?;
        if preflight.status == SkillsMigrationStatus::NotRequired
            && self.db.get_active_skills_migration_run()?.is_none()
        {
            return Ok(SkillsMigrationExecutionResult {
                outcome: SkillsMigrationExecutionOutcome::Completed,
                page_mode: SkillsMigrationPageMode::Writable,
                progress: SkillsMigrationProgress {
                    completed_items: 0,
                    total_items: 0,
                },
                items: Vec::new(),
                backup: None,
            });
        }
        if preflight.observation_token != intent.observation_token {
            return Ok(SkillsMigrationExecutionResult {
                outcome: SkillsMigrationExecutionOutcome::StaleObservation,
                page_mode: SkillsMigrationPageMode::ReadOnly,
                progress: SkillsMigrationProgress {
                    completed_items: 0,
                    total_items: 0,
                },
                items: Vec::new(),
                backup: None,
            });
        }
        let has_unresolved_plan_item = preflight.plan.iter().any(|item| {
            item.disposition == SkillsMigrationDisposition::UserResolve
                && (item.action != SkillsMigrationAction::PreserveUnsupportedConsumerFiles
                    || !intent.preserve_unsupported_consumer_files)
        });
        if preflight.status != SkillsMigrationStatus::DecisionNeeded
            || !preflight.backup.ready
            || has_unresolved_plan_item
        {
            return Ok(SkillsMigrationExecutionResult {
                outcome: SkillsMigrationExecutionOutcome::Blocked,
                page_mode: preflight.page_mode,
                progress: SkillsMigrationProgress {
                    completed_items: 0,
                    total_items: 0,
                },
                items: Vec::new(),
                backup: None,
            });
        }
        self.prepare_and_execute(preflight)
    }

    pub fn resume(&self) -> Result<SkillsMigrationExecutionResult> {
        let _migration_guard = Self::lock()?;
        let _library_guard =
            crate::services::skill::LibrarySkillAcquisitionService::lock_for_composite()?;
        let _deployment_guard =
            crate::services::skill_deployment::SkillDeploymentService::lock_for_composite()?;
        let run = self
            .db
            .get_active_skills_migration_run()?
            .ok_or_else(|| anyhow!("No resumable Skills migration exists"))?;
        self.execute_run(run, true)
    }

    pub fn restore(
        &self,
        intent: SkillsMigrationRestoreIntent,
    ) -> Result<SkillsMigrationExecutionResult> {
        let _migration_guard = Self::lock()?;
        let _library_guard = LibrarySkillAcquisitionService::lock_for_composite()?;
        let _deployment_guard = SkillDeploymentService::lock_for_composite()?;
        self.restore_locked(&intent.backup_id)
    }

    fn prepare_and_execute(
        &self,
        preflight: SkillsMigrationPreflight,
    ) -> Result<SkillsMigrationExecutionResult> {
        if let Some(active) = self.db.get_active_skills_migration_run()? {
            if active.accepted_observation_token != preflight.observation_token {
                return Ok(blocked_result());
            }
            return self.execute_run(active, true);
        }

        let plan = compile_execution_plan(&preflight.plan)?;
        let now = Utc::now().timestamp_millis();
        let run_id = uuid::Uuid::new_v4().to_string();
        let plan_hash = plan_hash(&plan)?;
        let run = SkillsMigrationRunRecord {
            id: run_id.clone(),
            accepted_observation_token: preflight.observation_token.clone(),
            resume_token: uuid::Uuid::new_v4().to_string(),
            state: "prepared".to_string(),
            database_backup_filename: None,
            content_backup_root: None,
            plan_hash,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        let items = plan
            .iter()
            .enumerate()
            .map(|(ordinal, item)| journal_item(&run_id, ordinal as u32, item))
            .collect::<Result<Vec<_>>>()?;

        // The journal captures filesystem identity that is intentionally not
        // exposed in the display-only preview DTO. Re-run the preview after
        // capturing it so a race between the accepted preview and journal
        // construction is rejected before either the journal or backup exists.
        let revalidated = inspect_locked(self.db.clone())?;
        if revalidated.observation_token != preflight.observation_token {
            return Ok(stale_result());
        }
        self.db
            .insert_skills_migration_run_with_items(&run, &items)?;

        match self.create_verified_backup(&run, &items) {
            Ok((database_backup_filename, content_backup_root)) => {
                self.db.update_skills_migration_run(
                    &run.id,
                    "running",
                    Some(&database_backup_filename),
                    Some(&content_backup_root),
                    Utc::now().timestamp_millis(),
                    None,
                )?;
            }
            Err(error) => {
                log::warn!("Skills migration backup creation failed: {error}");
                self.db.delete_skills_migration_run(&run.id)?;
                return Ok(blocked_result());
            }
        }

        let current = self
            .db
            .get_active_skills_migration_run()?
            .ok_or_else(|| anyhow!("prepared migration journal disappeared"))?;
        self.execute_run(current, false)
    }

    fn execute_run(
        &self,
        run: SkillsMigrationRunRecord,
        resumed: bool,
    ) -> Result<SkillsMigrationExecutionResult> {
        let items = self.db.list_skills_migration_items(&run.id)?;
        if !migration_backup_is_verified(&run, &items) {
            let mutation_may_have_started = items.iter().any(|item| {
                matches!(
                    item.state.as_str(),
                    "in_progress" | "completed" | "recovery_required" | "rolled_back"
                )
            });
            if mutation_may_have_started {
                self.db.update_skills_migration_run(
                    &run.id,
                    "recovery_required",
                    None,
                    None,
                    Utc::now().timestamp_millis(),
                    None,
                )?;
                return self
                    .result_for_run(&run, SkillsMigrationExecutionOutcome::RecoveryRequired);
            }
            cleanup_incomplete_backup(&run);
            self.db.delete_skills_migration_run(&run.id)?;
            return Ok(blocked_result());
        }
        self.db.update_skills_migration_run(
            &run.id,
            "running",
            None,
            None,
            Utc::now().timestamp_millis(),
            None,
        )?;
        let mut blocked_directories = std::collections::BTreeSet::new();
        let mut saw_blocked = false;
        for item in items {
            if item.action == "finalize" && saw_blocked {
                self.db.update_skills_migration_item(
                    &run.id,
                    item.ordinal,
                    SkillsMigrationItemUpdate {
                        state: "blocked",
                        library_skill_id: item.library_skill_id.as_deref(),
                        detail_code: Some("partial_batch"),
                        started_at: item.started_at,
                        completed_at: Some(Utc::now().timestamp_millis()),
                    },
                )?;
                continue;
            }
            if item
                .directory
                .as_ref()
                .is_some_and(|directory| blocked_directories.contains(directory))
            {
                self.db.update_skills_migration_item(
                    &run.id,
                    item.ordinal,
                    SkillsMigrationItemUpdate {
                        state: "blocked",
                        library_skill_id: item.library_skill_id.as_deref(),
                        detail_code: Some("partial_batch"),
                        started_at: item.started_at,
                        completed_at: Some(Utc::now().timestamp_millis()),
                    },
                )?;
                saw_blocked = true;
                continue;
            }
            if item.state == "completed" {
                if resumed && !self.item_postcondition_holds(&item)? {
                    self.db.update_skills_migration_item(
                        &run.id,
                        item.ordinal,
                        SkillsMigrationItemUpdate {
                            state: "blocked",
                            library_skill_id: item.library_skill_id.as_deref(),
                            detail_code: Some("postcondition_drift"),
                            started_at: item.started_at,
                            completed_at: Some(Utc::now().timestamp_millis()),
                        },
                    )?;
                    self.record_item_activity(
                        &run,
                        &item,
                        item.library_skill_id.as_deref(),
                        crate::services::activity::ActivityOutcome::Blocked,
                        crate::services::activity::ActivityDetailCode::StaleObservation,
                        resumed,
                    );
                    if let Some(directory) = item.directory {
                        blocked_directories.insert(directory);
                    }
                    saw_blocked = true;
                    continue;
                }
                if resumed {
                    self.db.update_skills_migration_item(
                        &run.id,
                        item.ordinal,
                        SkillsMigrationItemUpdate {
                            state: "completed",
                            library_skill_id: item.library_skill_id.as_deref(),
                            detail_code: Some("already_in_sync"),
                            started_at: item.started_at,
                            completed_at: item.completed_at,
                        },
                    )?;
                }
                continue;
            }
            if item.state == "in_progress" && self.item_postcondition_holds(&item)? {
                self.db.update_skills_migration_item(
                    &run.id,
                    item.ordinal,
                    SkillsMigrationItemUpdate {
                        state: "completed",
                        library_skill_id: item.library_skill_id.as_deref(),
                        detail_code: Some("already_in_sync"),
                        started_at: item.started_at,
                        completed_at: Some(Utc::now().timestamp_millis()),
                    },
                )?;
                continue;
            }
            if item.state == "recovery_required" {
                self.db.update_skills_migration_run(
                    &run.id,
                    "recovery_required",
                    None,
                    None,
                    Utc::now().timestamp_millis(),
                    None,
                )?;
                return self
                    .result_for_run(&run, SkillsMigrationExecutionOutcome::RecoveryRequired);
            }
            let now = Utc::now().timestamp_millis();
            self.db.update_skills_migration_item(
                &run.id,
                item.ordinal,
                SkillsMigrationItemUpdate {
                    state: "in_progress",
                    library_skill_id: item.library_skill_id.as_deref(),
                    detail_code: None,
                    started_at: Some(now),
                    completed_at: None,
                },
            )?;

            #[cfg(debug_assertions)]
            if should_interrupt_before(&item.action) {
                self.db.update_skills_migration_item(
                    &run.id,
                    item.ordinal,
                    SkillsMigrationItemUpdate {
                        state: "pending",
                        library_skill_id: item.library_skill_id.as_deref(),
                        detail_code: Some("interrupted"),
                        started_at: None,
                        completed_at: None,
                    },
                )?;
                self.db
                    .update_skills_migration_run(&run.id, "blocked", None, None, now, None)?;
                return self.result_for_run(&run, SkillsMigrationExecutionOutcome::Resumable);
            }

            match self.execute_item(&run, &item) {
                Ok(library_skill_id) => {
                    #[cfg(debug_assertions)]
                    let completion_failed = should_fail_journal_completion(&item.action);
                    #[cfg(not(debug_assertions))]
                    let completion_failed = false;
                    let completion = if completion_failed {
                        Err(crate::error::AppError::Database(
                            "injected migration journal completion failure".to_string(),
                        ))
                    } else {
                        self.db.update_skills_migration_item(
                            &run.id,
                            item.ordinal,
                            SkillsMigrationItemUpdate {
                                state: "completed",
                                library_skill_id: library_skill_id.as_deref(),
                                detail_code: if resumed {
                                    Some("already_in_sync")
                                } else {
                                    None
                                },
                                started_at: None,
                                completed_at: Some(Utc::now().timestamp_millis()),
                            },
                        )
                    };
                    if let Err(error) = completion {
                        log::warn!("Skills migration item completion journal failed: {error}");
                        let _ = self.db.update_skills_migration_run(
                            &run.id,
                            "blocked",
                            None,
                            None,
                            Utc::now().timestamp_millis(),
                            None,
                        );
                        return self
                            .result_for_run(&run, SkillsMigrationExecutionOutcome::Resumable);
                    }
                    self.record_item_activity(
                        &run,
                        &item,
                        library_skill_id.as_deref(),
                        crate::services::activity::ActivityOutcome::Success,
                        crate::services::activity::ActivityDetailCode::None,
                        resumed,
                    );
                }
                Err(ItemFailure::Blocked(detail)) => {
                    self.db.update_skills_migration_item(
                        &run.id,
                        item.ordinal,
                        SkillsMigrationItemUpdate {
                            state: "blocked",
                            library_skill_id: item.library_skill_id.as_deref(),
                            detail_code: Some(detail),
                            started_at: None,
                            completed_at: Some(Utc::now().timestamp_millis()),
                        },
                    )?;
                    self.db.update_skills_migration_run(
                        &run.id,
                        "blocked",
                        None,
                        None,
                        Utc::now().timestamp_millis(),
                        None,
                    )?;
                    self.record_item_activity(
                        &run,
                        &item,
                        item.library_skill_id.as_deref(),
                        crate::services::activity::ActivityOutcome::Blocked,
                        detail_code(detail),
                        resumed,
                    );
                    if let Some(directory) = item.directory {
                        blocked_directories.insert(directory);
                    }
                    saw_blocked = true;
                }
                Err(ItemFailure::RecoveryRequired(detail)) => {
                    self.db.update_skills_migration_item(
                        &run.id,
                        item.ordinal,
                        SkillsMigrationItemUpdate {
                            state: "recovery_required",
                            library_skill_id: item.library_skill_id.as_deref(),
                            detail_code: Some(detail),
                            started_at: None,
                            completed_at: Some(Utc::now().timestamp_millis()),
                        },
                    )?;
                    self.db.update_skills_migration_run(
                        &run.id,
                        "recovery_required",
                        None,
                        None,
                        Utc::now().timestamp_millis(),
                        None,
                    )?;
                    self.record_item_activity(
                        &run,
                        &item,
                        item.library_skill_id.as_deref(),
                        crate::services::activity::ActivityOutcome::CompensationFailed,
                        detail_code(detail),
                        resumed,
                    );
                    self.record_activity(false, resumed);
                    return self
                        .result_for_run(&run, SkillsMigrationExecutionOutcome::RecoveryRequired);
                }
                #[cfg(debug_assertions)]
                Err(ItemFailure::Interrupted) => {
                    self.db.update_skills_migration_item(
                        &run.id,
                        item.ordinal,
                        SkillsMigrationItemUpdate {
                            state: "pending",
                            library_skill_id: item.library_skill_id.as_deref(),
                            detail_code: Some("interrupted"),
                            started_at: None,
                            completed_at: None,
                        },
                    )?;
                    self.db.update_skills_migration_run(
                        &run.id,
                        "blocked",
                        None,
                        None,
                        Utc::now().timestamp_millis(),
                        None,
                    )?;
                    return self.result_for_run(&run, SkillsMigrationExecutionOutcome::Resumable);
                }
            }
        }
        if saw_blocked {
            self.db.update_skills_migration_run(
                &run.id,
                "blocked",
                None,
                None,
                Utc::now().timestamp_millis(),
                None,
            )?;
            self.record_activity(false, resumed);
            return self.result_for_run(&run, SkillsMigrationExecutionOutcome::Blocked);
        }
        let completed_at = Utc::now().timestamp_millis();
        self.db.update_skills_migration_run(
            &run.id,
            "completed",
            None,
            None,
            completed_at,
            Some(completed_at),
        )?;
        if let Some(parent) = run
            .content_backup_root
            .as_deref()
            .map(Path::new)
            .and_then(Path::parent)
        {
            prune_migration_backup_roots(&self.db, parent, &run.id)?;
        }
        let completed = SkillsMigrationRunRecord {
            state: "completed".to_string(),
            updated_at: completed_at,
            completed_at: Some(completed_at),
            ..run
        };
        self.record_activity(true, resumed);
        self.result_for_run(&completed, SkillsMigrationExecutionOutcome::Completed)
    }

    fn item_postcondition_holds(&self, item: &SkillsMigrationItemRecord) -> Result<bool> {
        let directory = item.directory.as_deref();
        match item.action.as_str() {
            "move_to_library" | "reuse_library" => {
                let Some(directory) = directory else {
                    return Ok(false);
                };
                let Some(skill) = self.db.get_library_skill_by_directory(directory)? else {
                    return Ok(false);
                };
                if item
                    .library_skill_id
                    .as_ref()
                    .is_some_and(|id| id != &skill.id)
                {
                    return Ok(false);
                }
                let path = LibrarySkillAcquisitionService::library_directory_path().join(directory);
                let hash = match LibrarySkillAcquisitionService::compute_library_hash(&path) {
                    Ok(hash) => hash,
                    Err(_) => return Ok(false),
                };
                let source_retired =
                    item.source_location
                        .as_deref()
                        .map(Path::new)
                        .is_none_or(|source| {
                            let parked = retired_source_staging(item, source);
                            source == path
                                || self.source_is_recorded_deployment(source, &skill)
                                || (matches!(
                                    fs::symlink_metadata(source),
                                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                                ) && matches!(
                                    fs::symlink_metadata(parked),
                                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                                ))
                        });
                Ok(hash == skill.content_hash
                    && item
                        .expected_fingerprint
                        .as_deref()
                        .and_then(content_hash_from_fingerprint)
                        == Some(hash.as_str())
                    && source_retired)
            }
            "create_global_deployment" => {
                let (Some(directory), Some(consumer)) =
                    (directory, parse_consumer(item.consumer.as_deref()))
                else {
                    return Ok(false);
                };
                let Some(skill) = self.db.get_library_skill_by_directory(directory)? else {
                    return Ok(false);
                };
                let inspected = SkillDeploymentService::new(self.db.clone()).inspect(
                    crate::services::skill_deployment::DeploymentQuery::for_target(
                        DeploymentTarget::global(consumer),
                    ),
                )?;
                Ok(inspected.items.iter().any(|observed| {
                    observed.library_skill_id == skill.id
                        && observed.status
                            == crate::services::skill_deployment::DeploymentStatus::InSync
                }))
            }
            "remove_legacy_codex_link" => {
                let Some(path) = item.source_location.as_deref().map(Path::new) else {
                    return Ok(false);
                };
                Ok(matches!(
                    fs::symlink_metadata(path),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                ))
            }
            "preserve_unsupported_consumer_files" => Ok(true),
            "finalize" => Ok(self.db.get_all_installed_skills()?.is_empty()
                && self
                    .db
                    .get_setting("skills_ssot_migration_pending")?
                    .is_none()
                && self
                    .db
                    .get_setting("skills_ssot_migration_snapshot")?
                    .is_none()),
            _ => Ok(false),
        }
    }

    fn execute_item(
        &self,
        run: &SkillsMigrationRunRecord,
        item: &SkillsMigrationItemRecord,
    ) -> std::result::Result<Option<String>, ItemFailure> {
        let result = match item.action.as_str() {
            "move_to_library" | "reuse_library" => self.execute_library_item(run, item),
            "create_global_deployment" => self.execute_deployment_item(item),
            "remove_legacy_codex_link" => self.execute_cleanup_item(item),
            "preserve_unsupported_consumer_files" => Ok(None),
            "finalize" => self
                .db
                .clear_legacy_skills_migration_state()
                .map(|_| None)
                .map_err(|_| ItemFailure::Blocked("database_failure")),
            _ => Err(ItemFailure::Blocked("invalid_input")),
        };
        result
    }

    fn source_is_recorded_deployment(
        &self,
        source: &Path,
        skill: &crate::services::skill::LibrarySkill,
    ) -> bool {
        [DeploymentConsumer::Claude, DeploymentConsumer::Codex]
            .into_iter()
            .any(|consumer| {
                let target = DeploymentTarget::global(consumer);
                let expected_target = match consumer {
                    DeploymentConsumer::Claude => {
                        get_home_dir().join(".claude/skills").join(&skill.directory)
                    }
                    DeploymentConsumer::Codex => {
                        get_home_dir().join(".agents/skills").join(&skill.directory)
                    }
                };
                source == expected_target
                    && self
                        .db
                        .get_skill_deployment(&skill.id, &target)
                        .ok()
                        .flatten()
                        .is_some()
                    && fs::canonicalize(source).ok()
                        == fs::canonicalize(
                            LibrarySkillAcquisitionService::library_directory_path()
                                .join(&skill.directory),
                        )
                        .ok()
            })
    }

    fn execute_library_item(
        &self,
        run: &SkillsMigrationRunRecord,
        item: &SkillsMigrationItemRecord,
    ) -> std::result::Result<Option<String>, ItemFailure> {
        let directory = item
            .directory
            .as_deref()
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        if let Some(existing) = self
            .db
            .get_library_skill_by_directory(directory)
            .map_err(|_| ItemFailure::Blocked("database_failure"))?
        {
            let path = LibrarySkillAcquisitionService::library_directory_path().join(directory);
            let hash = LibrarySkillAcquisitionService::compute_library_hash(&path)
                .map_err(|_| ItemFailure::Blocked("validation_failure"))?;
            if item
                .expected_fingerprint
                .as_deref()
                .and_then(content_hash_from_fingerprint)
                == Some(hash.as_str())
            {
                let source = item.source_location.as_deref().map(Path::new);
                if item.action == "move_to_library" || source.is_some_and(|source| source != path) {
                    self.retire_legacy_source(item, &path)?;
                }
                return Ok(Some(existing.id));
            }
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        let source = item
            .source_location
            .as_deref()
            .map(Path::new)
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        let library_path = LibrarySkillAcquisitionService::library_directory_path().join(directory);
        if source == library_path {
            self.require_expected_source_fingerprint(item, source)?;
            return LibrarySkillAcquisitionService::admit_existing_library_directory_locked(
                &self.db, directory,
            )
            .map(|skill| Some(skill.id))
            .map_err(|_| ItemFailure::Blocked("database_failure"));
        }
        if !source.exists() {
            let backup = content_backup_source(run, item.ordinal);
            if backup.is_dir() {
                return LibrarySkillAcquisitionService::acquire_from_directory_locked(
                    &self.db,
                    &backup,
                    local_source(),
                    Some(directory),
                )
                .map(|skill| Some(skill.id))
                .map_err(|_| ItemFailure::Blocked("filesystem_failure"));
            }
            return Err(ItemFailure::Blocked("filesystem_failure"));
        }
        self.require_expected_source_fingerprint(item, source)?;
        let acquired = LibrarySkillAcquisitionService::acquire_from_directory_locked(
            &self.db,
            source,
            local_source(),
            Some(directory),
        )
        .map_err(|_| ItemFailure::Blocked("filesystem_failure"))?;
        #[cfg(debug_assertions)]
        if should_fail_compensation("move_to_library") {
            return Err(ItemFailure::RecoveryRequired("compensation_failure"));
        }
        if let Err(failure) = self.retire_legacy_source(item, &library_path) {
            #[cfg(debug_assertions)]
            if matches!(failure, ItemFailure::Interrupted) {
                return Err(failure);
            }
            return match compensate_created_library(&self.db, &acquired.id, &library_path) {
                Ok(()) => Err(failure),
                Err(error) => {
                    log::warn!("migration Library compensation failed: {error}");
                    Err(ItemFailure::RecoveryRequired("compensation_failure"))
                }
            };
        }
        Ok(Some(acquired.id))
    }

    fn require_expected_source_fingerprint(
        &self,
        item: &SkillsMigrationItemRecord,
        source: &Path,
    ) -> std::result::Result<(), ItemFailure> {
        let current =
            path_fingerprint(source).map_err(|_| ItemFailure::Blocked("validation_failure"))?;
        if item.expected_fingerprint.as_deref() != Some(current.as_str()) {
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        Ok(())
    }

    fn retire_legacy_source(
        &self,
        item: &SkillsMigrationItemRecord,
        library_path: &Path,
    ) -> std::result::Result<(), ItemFailure> {
        let source = item
            .source_location
            .as_deref()
            .map(Path::new)
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        if source == library_path {
            return Ok(());
        }
        let parked = retired_source_staging(item, source);
        match fs::symlink_metadata(source) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return match fs::symlink_metadata(&parked) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        self.require_expected_source_fingerprint(item, &parked)?;
                        fs::remove_dir_all(&parked)
                            .map_err(|_| ItemFailure::RecoveryRequired("compensation_failure"))?;
                        Ok(())
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(ItemFailure::RecoveryRequired("compensation_failure")),
                }
            }
            Err(_) => return Err(ItemFailure::Blocked("filesystem_failure")),
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(ItemFailure::Blocked("target_conflict"));
            }
            Ok(_) => {}
        }
        let source_hash = LibrarySkillAcquisitionService::compute_library_hash(source)
            .map_err(|_| ItemFailure::Blocked("validation_failure"))?;
        let library_hash = LibrarySkillAcquisitionService::compute_library_hash(library_path)
            .map_err(|_| ItemFailure::Blocked("missing_library"))?;
        if source_hash != library_hash
            || item
                .expected_fingerprint
                .as_deref()
                .and_then(content_hash_from_fingerprint)
                != Some(source_hash.as_str())
        {
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        if fs::symlink_metadata(&parked).is_ok() {
            return Err(ItemFailure::RecoveryRequired("compensation_failure"));
        }
        fs::rename(source, &parked).map_err(|_| ItemFailure::Blocked("filesystem_failure"))?;
        #[cfg(debug_assertions)]
        if should_interrupt_after_source_retire() {
            return Err(ItemFailure::Interrupted);
        }
        if let Err(error) = fs::remove_dir_all(&parked) {
            let restored = fs::rename(&parked, source);
            return if restored.is_ok() {
                Err(ItemFailure::Blocked("filesystem_failure"))
            } else {
                log::warn!("migration source cleanup and compensation failed: {error}");
                Err(ItemFailure::RecoveryRequired("compensation_failure"))
            };
        }
        Ok(())
    }

    fn execute_deployment_item(
        &self,
        item: &SkillsMigrationItemRecord,
    ) -> std::result::Result<Option<String>, ItemFailure> {
        let directory = item
            .directory
            .as_deref()
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        let skill = self
            .db
            .get_library_skill_by_directory(directory)
            .map_err(|_| ItemFailure::Blocked("database_failure"))?
            .ok_or(ItemFailure::Blocked("missing_library"))?;
        let consumer = parse_consumer(item.consumer.as_deref())
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        let target = DeploymentTarget::global(consumer);
        let service = SkillDeploymentService::new(self.db.clone());
        let applied = service
            .apply_one_for_composite(&DeploymentIntent::Deploy {
                library_skill_id: skill.id.clone(),
                target: target.clone(),
            })
            .map_err(|error| {
                if error.to_string().contains("recovery required") {
                    ItemFailure::RecoveryRequired("compensation_failure")
                } else {
                    ItemFailure::Blocked("filesystem_failure")
                }
            })?;
        match applied.outcome {
            DeploymentMutationOutcome::Applied | DeploymentMutationOutcome::AlreadyInSync => {
                Ok(Some(skill.id))
            }
            DeploymentMutationOutcome::Conflict => {
                let target_path = item
                    .target_location
                    .as_deref()
                    .map(Path::new)
                    .ok_or(ItemFailure::Blocked("invalid_input"))?;
                let fingerprint = link_fingerprint_path(target_path)
                    .ok_or(ItemFailure::Blocked("target_conflict"))?;
                let expected_library =
                    LibrarySkillAcquisitionService::library_directory_path().join(directory);
                if item.expected_fingerprint.as_deref() != Some(fingerprint.as_str())
                    || fs::canonicalize(target_path).ok() != fs::canonicalize(expected_library).ok()
                {
                    return Err(ItemFailure::Blocked("target_conflict"));
                }
                let adopted = service
                    .adopt_exact_link_for_composite(&skill.id, &target)
                    .map_err(|_| ItemFailure::Blocked("database_failure"))?;
                match adopted.outcome {
                    DeploymentMutationOutcome::Applied
                    | DeploymentMutationOutcome::AlreadyInSync => Ok(Some(skill.id)),
                    DeploymentMutationOutcome::RecoveryRequired => {
                        Err(ItemFailure::RecoveryRequired("compensation_failure"))
                    }
                    _ => Err(ItemFailure::Blocked("target_conflict")),
                }
            }
            DeploymentMutationOutcome::RecoveryRequired => {
                Err(ItemFailure::RecoveryRequired("compensation_failure"))
            }
            _ => Err(ItemFailure::Blocked("target_conflict")),
        }
    }

    fn execute_cleanup_item(
        &self,
        item: &SkillsMigrationItemRecord,
    ) -> std::result::Result<Option<String>, ItemFailure> {
        let path = item
            .source_location
            .as_deref()
            .map(Path::new)
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ItemFailure::Blocked("filesystem_failure")),
        };
        if !metadata.file_type().is_symlink() {
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        let raw_target =
            fs::read_link(path).map_err(|_| ItemFailure::Blocked("target_conflict"))?;
        let fingerprint =
            link_fingerprint_path(path).ok_or(ItemFailure::Blocked("target_conflict"))?;
        if item.expected_fingerprint.as_deref() != Some(fingerprint.as_str()) {
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        let directory = item
            .directory
            .as_deref()
            .ok_or(ItemFailure::Blocked("invalid_input"))?;
        let resolved = resolve_link_target(path, &raw_target)
            .ok_or(ItemFailure::Blocked("target_conflict"))?;
        let target_is_trusted = trusted_cleanup_targets(directory).iter().any(|candidate| {
            resolved == *candidate
                || matches!(
                    (fs::canonicalize(&resolved), fs::canonicalize(candidate)),
                    (Ok(left), Ok(right)) if left == right
                )
        });
        if !target_is_trusted {
            return Err(ItemFailure::Blocked("target_conflict"));
        }
        fs::remove_file(path)
            .map(|_| None)
            .map_err(|_| ItemFailure::Blocked("filesystem_failure"))
    }

    fn create_verified_backup(
        &self,
        run: &SkillsMigrationRunRecord,
        items: &[SkillsMigrationItemRecord],
    ) -> Result<(String, String)> {
        let root = crate::config::get_app_config_dir()
            .join("skills-migration-backups")
            .join(&run.id);
        let attempt = (|| {
            fs::create_dir_all(root.join("content"))?;
            let database_backup = root.join("database.db");
            // Persist the immutable backup identities before taking the snapshot so
            // the protected database itself remains a complete recovery locator.
            self.db.update_skills_migration_run(
                &run.id,
                "prepared",
                Some(&database_backup.to_string_lossy()),
                Some(&root.to_string_lossy()),
                Utc::now().timestamp_millis(),
                None,
            )?;
            self.db.backup_database_snapshot_file(&database_backup)?;
            fs::File::open(&database_backup)
                .with_context(|| format!("verify database backup {}", database_backup.display()))?;
            for item in items.iter().filter(|item| {
                matches!(
                    item.action.as_str(),
                    "move_to_library" | "reuse_library" | "remove_legacy_codex_link"
                )
            }) {
                let Some(source) = item.source_location.as_deref().map(Path::new) else {
                    return Err(anyhow!(
                        "migration backup source is missing from its journal"
                    ));
                };
                #[cfg(debug_assertions)]
                if should_remove_source_before_backup(&item.action) {
                    remove_path_for_test(source)?;
                }
                fs::symlink_metadata(source).with_context(|| {
                    format!(
                        "required migration backup source vanished: {}",
                        source.display()
                    )
                })?;
                let before = path_fingerprint(source)?;
                if item.expected_fingerprint.as_deref() != Some(before.as_str()) {
                    return Err(anyhow!("migration backup source observation changed"));
                }
                let destination = content_backup_source_path(&root, item.ordinal);
                backup_path(source, &destination)?;
                verify_backup(source, &destination)?;
                if path_fingerprint(source)? != before {
                    return Err(anyhow!("migration backup source changed during backup"));
                }
            }
            let marker = verified_backup_marker(&root);
            let manifest = build_backup_manifest(run, items, &database_backup, &root)?;
            fs::write(&marker, serde_json::to_vec(&manifest)?)?;
            fs::File::open(&marker)?.sync_all()?;
            prune_migration_backup_roots(&self.db, root.parent().unwrap_or(&root), &run.id)?;
            Ok((
                database_backup.to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
            ))
        })();
        if attempt.is_err() {
            let _ = fs::remove_dir_all(&root);
        }
        attempt
    }

    fn result_for_run(
        &self,
        run: &SkillsMigrationRunRecord,
        outcome: SkillsMigrationExecutionOutcome,
    ) -> Result<SkillsMigrationExecutionResult> {
        let rows = self.db.list_skills_migration_items(&run.id)?;
        let completed_items = rows.iter().filter(|item| item.state == "completed").count() as u32;
        let items = rows.iter().filter_map(result_item).collect::<Vec<_>>();
        Ok(SkillsMigrationExecutionResult {
            outcome,
            page_mode: if outcome == SkillsMigrationExecutionOutcome::Completed {
                SkillsMigrationPageMode::Writable
            } else {
                SkillsMigrationPageMode::ReadOnly
            },
            progress: SkillsMigrationProgress {
                completed_items,
                total_items: rows.len() as u32,
            },
            items,
            backup: run.database_backup_filename.as_ref().map(|database| {
                SkillsMigrationBackupReference {
                    backup_id: run.id.clone(),
                    created_at: run.created_at,
                    restore_available: Path::new(database).is_file()
                        && migration_backup_is_verified(run, &rows),
                }
            }),
        })
    }

    pub(crate) fn inspect_run(
        &self,
        run: &SkillsMigrationRunRecord,
        outcome: SkillsMigrationExecutionOutcome,
    ) -> Result<SkillsMigrationExecutionResult> {
        self.result_for_run(run, outcome)
    }

    fn record_activity(&self, success: bool, resumed: bool) {
        crate::services::activity::ActivityRecorder::new(self.db.clone()).record_best_effort(
            crate::services::activity::ActivityEventInput {
                operation: crate::services::activity::ActivityOperation::Migration,
                reason: if resumed {
                    crate::services::activity::ActivityReason::Resume
                } else {
                    crate::services::activity::ActivityReason::Migrate
                },
                outcome: if success {
                    crate::services::activity::ActivityOutcome::Success
                } else {
                    crate::services::activity::ActivityOutcome::Failed
                },
                actor: crate::services::activity::ActivityActor::Migration,
                trigger: if resumed {
                    crate::services::activity::ActivityTrigger::Resume
                } else {
                    crate::services::activity::ActivityTrigger::Command
                },
                target: Default::default(),
                batch: None,
                detail_code: crate::services::activity::ActivityDetailCode::None,
            },
        );
    }

    fn record_restore_activity(&self) {
        crate::services::activity::ActivityRecorder::new(self.db.clone()).record_best_effort(
            crate::services::activity::ActivityEventInput {
                operation: crate::services::activity::ActivityOperation::Removal,
                reason: crate::services::activity::ActivityReason::CompensationRestore,
                outcome: crate::services::activity::ActivityOutcome::RolledBack,
                actor: crate::services::activity::ActivityActor::Migration,
                trigger: crate::services::activity::ActivityTrigger::Command,
                target: Default::default(),
                batch: None,
                detail_code: crate::services::activity::ActivityDetailCode::None,
            },
        );
    }

    fn record_item_activity(
        &self,
        run: &SkillsMigrationRunRecord,
        item: &SkillsMigrationItemRecord,
        library_skill_id: Option<&str>,
        outcome: crate::services::activity::ActivityOutcome,
        detail_code: crate::services::activity::ActivityDetailCode,
        resumed: bool,
    ) {
        crate::services::activity::ActivityRecorder::new(self.db.clone()).record_best_effort(
            crate::services::activity::ActivityEventInput {
                operation: crate::services::activity::ActivityOperation::Migration,
                reason: crate::services::activity::ActivityReason::MigrateItem,
                outcome,
                actor: crate::services::activity::ActivityActor::Migration,
                trigger: if resumed {
                    crate::services::activity::ActivityTrigger::Resume
                } else {
                    crate::services::activity::ActivityTrigger::Command
                },
                target: crate::services::activity::ActivityTarget {
                    library_skill_id: library_skill_id.map(str::to_string),
                    consumer: parse_consumer(item.consumer.as_deref()),
                    workspace_kind: item
                        .consumer
                        .as_ref()
                        .map(|_| crate::services::skill_deployment::WorkspaceKind::Global),
                    ..Default::default()
                },
                batch: Some(crate::services::activity::ActivityBatchContext {
                    batch_id: run.id.clone(),
                    item_index: item.ordinal,
                    item_count: self
                        .db
                        .list_skills_migration_items(&run.id)
                        .map(|items| items.len() as u32)
                        .unwrap_or(1),
                }),
                detail_code,
            },
        );
    }

    fn restore_locked(&self, backup_id: &str) -> Result<SkillsMigrationExecutionResult> {
        if backup_id.is_empty()
            || backup_id.contains('/')
            || backup_id.contains('\\')
            || backup_id.contains("..")
        {
            return Err(anyhow!("invalid Skills migration backup identity"));
        }
        let run = self
            .db
            .get_skills_migration_run_by_backup_id(backup_id)?
            .ok_or_else(|| anyhow!("Skills migration backup does not exist"))?;
        if self
            .db
            .get_active_skills_migration_run()?
            .is_some_and(|active| active.id != run.id)
        {
            return Ok(blocked_result());
        }
        let database_backup = run
            .database_backup_filename
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| anyhow!("database backup is unavailable"))?;
        let items = self.db.list_skills_migration_items(&run.id)?;
        let root = run
            .content_backup_root
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| anyhow!("content backup is unavailable"))?;
        if !root.is_dir() || !migration_backup_is_verified(&run, &items) {
            self.db.update_skills_migration_run(
                &run.id,
                "recovery_required",
                None,
                None,
                Utc::now().timestamp_millis(),
                None,
            )?;
            return self.result_for_run(&run, SkillsMigrationExecutionOutcome::RecoveryRequired);
        }
        if let Err(error) = validate_restore_safety(&self.db, &items, root) {
            self.db.update_skills_migration_run(
                &run.id,
                "recovery_required",
                None,
                None,
                Utc::now().timestamp_millis(),
                None,
            )?;
            return Err(error);
        }
        let recovery_run = SkillsMigrationRunRecord {
            state: "recovery_required".to_string(),
            updated_at: Utc::now().timestamp_millis(),
            completed_at: None,
            ..run.clone()
        };
        let recovery_items = items
            .iter()
            .cloned()
            .map(|mut item| {
                if item.state == "in_progress" {
                    item.state = "recovery_required".to_string();
                }
                item
            })
            .collect::<Vec<_>>();
        self.db
            .replace_skills_migration_run_with_items(&recovery_run, &recovery_items)?;
        restore_remove_migration_outputs(&self.db, &items)?;
        for item in &items {
            let Some(original) = item.source_location.as_deref().map(Path::new) else {
                continue;
            };
            let parked = retired_source_staging(item, original);
            match fs::symlink_metadata(&parked) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    fs::remove_dir_all(&parked)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(anyhow!("migration retired staging is unsafe to remove")),
                Err(error) => return Err(error.into()),
            }
            let backup = content_backup_source_path(root, item.ordinal);
            if !backup.exists() && fs::symlink_metadata(&backup).is_err() {
                continue;
            }
            restore_backup_path(&backup, original, item)?;
        }
        restore_database_file(&self.db, database_backup)?;
        #[cfg(debug_assertions)]
        if should_interrupt_after_database_restore() {
            return Err(anyhow!("injected interruption after database restore"));
        }
        let restored_at = Utc::now().timestamp_millis();
        let restored = SkillsMigrationRunRecord {
            state: "restored".to_string(),
            updated_at: restored_at,
            completed_at: Some(restored_at),
            ..run
        };
        let restored_items = items
            .into_iter()
            .map(|mut item| {
                item.state = "rolled_back".to_string();
                item.completed_at = Some(restored_at);
                item
            })
            .collect::<Vec<_>>();
        self.db
            .replace_skills_migration_run_with_items(&restored, &restored_items)?;
        self.record_restore_activity();
        self.result_for_run(&restored, SkillsMigrationExecutionOutcome::Restored)
    }
}

#[derive(Debug)]
enum ItemFailure {
    Blocked(&'static str),
    RecoveryRequired(&'static str),
    #[cfg(debug_assertions)]
    Interrupted,
}

fn blocked_result() -> SkillsMigrationExecutionResult {
    SkillsMigrationExecutionResult {
        outcome: SkillsMigrationExecutionOutcome::Blocked,
        page_mode: SkillsMigrationPageMode::ReadOnly,
        progress: SkillsMigrationProgress {
            completed_items: 0,
            total_items: 0,
        },
        items: Vec::new(),
        backup: None,
    }
}

fn stale_result() -> SkillsMigrationExecutionResult {
    SkillsMigrationExecutionResult {
        outcome: SkillsMigrationExecutionOutcome::StaleObservation,
        page_mode: SkillsMigrationPageMode::ReadOnly,
        progress: SkillsMigrationProgress {
            completed_items: 0,
            total_items: 0,
        },
        items: Vec::new(),
        backup: None,
    }
}

fn local_source() -> LibrarySkillSource {
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

fn parse_consumer(raw: Option<&str>) -> Option<DeploymentConsumer> {
    match raw {
        Some("claude") => Some(DeploymentConsumer::Claude),
        Some("codex") => Some(DeploymentConsumer::Codex),
        _ => None,
    }
}

fn action_name(action: SkillsMigrationAction) -> &'static str {
    match action {
        SkillsMigrationAction::MoveToLibrary => "move_to_library",
        SkillsMigrationAction::ReuseLibrary => "reuse_library",
        SkillsMigrationAction::CreateGlobalDeployment => "create_global_deployment",
        SkillsMigrationAction::RemoveLegacyCodexLink => "remove_legacy_codex_link",
        SkillsMigrationAction::PreserveContent => "preserve_content",
        SkillsMigrationAction::PreserveUnsupportedConsumerFiles => {
            "preserve_unsupported_consumer_files"
        }
        SkillsMigrationAction::ResolveConflict => "resolve_conflict",
        SkillsMigrationAction::RepairPreflight => "repair_preflight",
        SkillsMigrationAction::Finalize => "finalize",
    }
}

fn action_from_name(raw: &str) -> Option<SkillsMigrationAction> {
    Some(match raw {
        "move_to_library" => SkillsMigrationAction::MoveToLibrary,
        "reuse_library" => SkillsMigrationAction::ReuseLibrary,
        "create_global_deployment" => SkillsMigrationAction::CreateGlobalDeployment,
        "remove_legacy_codex_link" => SkillsMigrationAction::RemoveLegacyCodexLink,
        "preserve_content" => SkillsMigrationAction::PreserveContent,
        "preserve_unsupported_consumer_files" => {
            SkillsMigrationAction::PreserveUnsupportedConsumerFiles
        }
        "resolve_conflict" => SkillsMigrationAction::ResolveConflict,
        "repair_preflight" => SkillsMigrationAction::RepairPreflight,
        "finalize" => SkillsMigrationAction::Finalize,
        _ => return None,
    })
}

fn reason_from_detail(raw: Option<&str>) -> Option<SkillsMigrationReason> {
    match raw {
        Some("already_in_sync") => Some(SkillsMigrationReason::AlreadyInLibrary),
        Some("target_conflict") => Some(SkillsMigrationReason::ForeignOrAmbiguous),
        Some("missing_library" | "filesystem_failure") => {
            Some(SkillsMigrationReason::MissingSource)
        }
        Some("validation_failure" | "invalid_input" | "database_failure") => {
            Some(SkillsMigrationReason::InvalidLegacyState)
        }
        Some("compensation_failure") => Some(SkillsMigrationReason::ContentConflict),
        Some("postcondition_drift") => Some(SkillsMigrationReason::ContentConflict),
        Some("migration_finalized") => Some(SkillsMigrationReason::MigrationFinalized),
        _ => None,
    }
}

fn detail_code(raw: &str) -> crate::services::activity::ActivityDetailCode {
    match raw {
        "target_conflict" => crate::services::activity::ActivityDetailCode::TargetConflict,
        "missing_library" => crate::services::activity::ActivityDetailCode::MissingLibrary,
        "filesystem_failure" => crate::services::activity::ActivityDetailCode::FilesystemFailure,
        "database_failure" => crate::services::activity::ActivityDetailCode::DatabaseFailure,
        "compensation_failure" => {
            crate::services::activity::ActivityDetailCode::CompensationFailure
        }
        "postcondition_drift" => crate::services::activity::ActivityDetailCode::StaleObservation,
        "validation_failure" => crate::services::activity::ActivityDetailCode::ValidationFailure,
        _ => crate::services::activity::ActivityDetailCode::InvalidInput,
    }
}

fn result_item(item: &SkillsMigrationItemRecord) -> Option<SkillsMigrationItemResult> {
    let action = action_from_name(&item.action)?;
    let outcome = match item.state.as_str() {
        "completed" if item.detail_code.as_deref() == Some("already_in_sync") => {
            SkillsMigrationItemOutcome::AlreadyCompleted
        }
        "completed" => SkillsMigrationItemOutcome::Completed,
        "rolled_back" => SkillsMigrationItemOutcome::RolledBack,
        "blocked" => SkillsMigrationItemOutcome::Blocked,
        "recovery_required" => SkillsMigrationItemOutcome::RecoveryRequired,
        _ => return None,
    };
    Some(SkillsMigrationItemResult {
        action,
        directory: item.directory.clone(),
        consumer: parse_consumer(item.consumer.as_deref()),
        outcome,
        reason: reason_from_detail(item.detail_code.as_deref()),
    })
}

fn compile_execution_plan(
    plan: &[SkillsMigrationPlanItem],
) -> Result<Vec<SkillsMigrationPlanItem>> {
    let mut perform = plan
        .iter()
        .filter(|item| {
            item.disposition == SkillsMigrationDisposition::Perform
                || item.action == SkillsMigrationAction::PreserveUnsupportedConsumerFiles
        })
        .cloned()
        .collect::<Vec<_>>();
    perform.sort_by(|left, right| {
        left.directory
            .cmp(&right.directory)
            .then(action_rank(left.action).cmp(&action_rank(right.action)))
            .then(consumer_rank(left.consumer).cmp(&consumer_rank(right.consumer)))
    });
    perform.push(SkillsMigrationPlanItem {
        disposition: SkillsMigrationDisposition::Perform,
        action: SkillsMigrationAction::Finalize,
        directory: None,
        consumer: None,
        from_location: None,
        to_location: None,
        reason: SkillsMigrationReason::MigrationFinalized,
        unsupported_consumers: Vec::new(),
    });
    Ok(perform)
}

fn action_rank(action: SkillsMigrationAction) -> u8 {
    match action {
        SkillsMigrationAction::PreserveUnsupportedConsumerFiles => 0,
        SkillsMigrationAction::MoveToLibrary | SkillsMigrationAction::ReuseLibrary => 1,
        SkillsMigrationAction::CreateGlobalDeployment => 2,
        SkillsMigrationAction::RemoveLegacyCodexLink => 3,
        SkillsMigrationAction::PreserveContent => 4,
        SkillsMigrationAction::ResolveConflict | SkillsMigrationAction::RepairPreflight => 5,
        SkillsMigrationAction::Finalize => 6,
    }
}

fn consumer_rank(consumer: Option<DeploymentConsumer>) -> u8 {
    match consumer {
        None => 0,
        Some(DeploymentConsumer::Claude) => 1,
        Some(DeploymentConsumer::Codex) => 2,
    }
}

fn plan_hash(plan: &[SkillsMigrationPlanItem]) -> Result<String> {
    let bytes = serde_json::to_vec(plan)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn journal_item(
    run_id: &str,
    ordinal: u32,
    item: &SkillsMigrationPlanItem,
) -> Result<SkillsMigrationItemRecord> {
    let source_fingerprint = item
        .from_location
        .as_deref()
        .map(Path::new)
        .map(path_fingerprint)
        .transpose()?;
    let expected_fingerprint = source_fingerprint.clone();
    Ok(SkillsMigrationItemRecord {
        run_id: run_id.to_string(),
        ordinal,
        item_key: format!(
            "{ordinal:06}:{}:{}:{}:{}",
            item.directory.as_deref().unwrap_or("_"),
            action_name(item.action),
            item.consumer
                .map(|consumer| match consumer {
                    DeploymentConsumer::Claude => "claude",
                    DeploymentConsumer::Codex => "codex",
                })
                .unwrap_or("_"),
            source_fingerprint.as_deref().unwrap_or("_")
        ),
        action: action_name(item.action).to_string(),
        directory: item.directory.clone(),
        consumer: item.consumer.map(|consumer| match consumer {
            DeploymentConsumer::Claude => "claude".to_string(),
            DeploymentConsumer::Codex => "codex".to_string(),
        }),
        source_location: item.from_location.clone(),
        target_location: item.to_location.clone(),
        expected_fingerprint,
        state: "pending".to_string(),
        library_skill_id: None,
        detail_code: None,
        started_at: None,
        completed_at: None,
    })
}

fn path_fingerprint(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return link_fingerprint_path(path)
            .ok_or_else(|| anyhow!("migration link fingerprint is unavailable"));
    }
    if metadata.is_dir() {
        let hash = LibrarySkillAcquisitionService::compute_library_hash(path)?;
        return Ok(format!("dir:{hash}:{}:{}", metadata.dev(), metadata.ino()));
    }
    let bytes = fs::read(path)?;
    Ok(format!(
        "file:sha256:{:x}:{}:{}",
        Sha256::digest(bytes),
        metadata.dev(),
        metadata.ino()
    ))
}

fn content_hash_from_fingerprint(fingerprint: &str) -> Option<&str> {
    fingerprint
        .strip_prefix("dir:")?
        .rsplit_once(':')?
        .0
        .rsplit_once(':')
        .map(|(hash, _)| hash)
}

fn physical_identity_matches(path: &Path, fingerprint: &str) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let mut parts = fingerprint.rsplitn(3, ':');
    let inode = parts.next().and_then(|value| value.parse::<u64>().ok());
    let device = parts.next().and_then(|value| value.parse::<u64>().ok());
    device == Some(metadata.dev()) && inode == Some(metadata.ino())
}

fn link_fingerprint_path(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let target = fs::read_link(path).ok()?;
    Some(format!(
        "link:{}:{}:{}",
        target.to_string_lossy(),
        metadata.dev(),
        metadata.ino()
    ))
}

fn resolve_link_target(path: &Path, target: &Path) -> Option<PathBuf> {
    let resolved = if target.is_absolute() {
        target.to_path_buf()
    } else {
        path.parent()?.join(target)
    };
    Some(normalize_lexical_path(&resolved))
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn trusted_cleanup_targets(directory: &str) -> [PathBuf; 2] {
    [
        LibrarySkillAcquisitionService::library_directory_path().join(directory),
        get_home_dir().join(".agents/skills").join(directory),
    ]
}

fn compensate_created_library(db: &Database, id: &str, path: &Path) -> Result<()> {
    let deleted = db.delete_library_skill(id)?;
    if !deleted {
        return Err(anyhow!(
            "migration Library row disappeared before compensation"
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(anyhow!("migration Library compensation target changed")),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn content_backup_source(run: &SkillsMigrationRunRecord, ordinal: u32) -> PathBuf {
    run.content_backup_root
        .as_deref()
        .map(PathBuf::from)
        .map(|root| content_backup_source_path(&root, ordinal))
        .unwrap_or_default()
}

fn content_backup_source_path(root: &Path, ordinal: u32) -> PathBuf {
    root.join("content").join(format!("{ordinal:06}"))
}

fn retired_source_staging(item: &SkillsMigrationItemRecord, source: &Path) -> PathBuf {
    let directory = item.directory.as_deref().unwrap_or("skill");
    source.with_file_name(format!(
        ".cc-switch-migrated-{directory}-{}-{:06}",
        item.run_id, item.ordinal
    ))
}

const MIGRATION_BACKUP_RETAIN_COUNT: usize = 20;

fn verified_backup_marker(root: &Path) -> PathBuf {
    root.join("backup-verified")
}

fn migration_backup_is_verified(
    run: &SkillsMigrationRunRecord,
    items: &[SkillsMigrationItemRecord],
) -> bool {
    let Some(database) = run.database_backup_filename.as_deref().map(Path::new) else {
        return false;
    };
    let Some(root) = run.content_backup_root.as_deref().map(Path::new) else {
        return false;
    };
    if !database.is_file() || !root.is_dir() {
        return false;
    }
    let manifest = fs::read(verified_backup_marker(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SkillsMigrationBackupManifest>(&bytes).ok());
    let Some(manifest) = manifest else {
        return false;
    };
    build_backup_manifest(run, items, database, root)
        .ok()
        .is_some_and(|actual| actual == manifest)
}

fn build_backup_manifest(
    run: &SkillsMigrationRunRecord,
    items: &[SkillsMigrationItemRecord],
    database: &Path,
    root: &Path,
) -> Result<SkillsMigrationBackupManifest> {
    let content = items
        .iter()
        .filter(|item| {
            matches!(
                item.action.as_str(),
                "move_to_library" | "reuse_library" | "remove_legacy_codex_link"
            )
        })
        .map(|item| {
            let backup = content_backup_source_path(root, item.ordinal);
            Ok(SkillsMigrationBackupContent {
                ordinal: item.ordinal,
                fingerprint: backup_payload_fingerprint(&backup)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SkillsMigrationBackupManifest {
        version: 1,
        plan_hash: run.plan_hash.clone(),
        database_sha256: file_sha256(database)?,
        content,
    })
}

fn backup_payload_fingerprint(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path)?;
        let mut hasher = Sha256::new();
        hasher.update(target.as_os_str().as_encoded_bytes());
        Ok(format!("link:{:x}", hasher.finalize()))
    } else if metadata.is_dir() {
        Ok(format!(
            "directory:{}",
            LibrarySkillAcquisitionService::compute_library_hash(path)?
        ))
    } else if metadata.is_file() {
        Ok(format!("file:{}", file_sha256(path)?))
    } else {
        Err(anyhow!("migration backup artifact has an unsupported type"))
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn cleanup_incomplete_backup(run: &SkillsMigrationRunRecord) {
    if let Some(root) = run.content_backup_root.as_deref().map(Path::new) {
        let _ = fs::remove_dir_all(root);
    }
}

fn prune_migration_backup_roots(db: &Database, parent: &Path, current_run_id: &str) -> Result<()> {
    let mut roots = match fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                metadata
                    .is_dir()
                    .then_some((metadata.modified().ok(), entry.path()))
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    roots.sort_by_key(|entry| entry.0);
    while roots.len() > MIGRATION_BACKUP_RETAIN_COUNT {
        let index = roots
            .iter()
            .position(|(_, path)| {
                let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                    return false;
                };
                id != current_run_id
                    && db
                        .get_skills_migration_run_by_backup_id(id)
                        .ok()
                        .flatten()
                        .is_some_and(|run| {
                            !matches!(
                                run.state.as_str(),
                                "prepared" | "running" | "blocked" | "recovery_required"
                            )
                        })
            })
            .or_else(|| {
                roots.iter().position(|(_, path)| {
                    path.file_name().and_then(|name| name.to_str()) != Some(current_run_id)
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|id| {
                                db.get_skills_migration_run_by_backup_id(id)
                                    .ok()
                                    .flatten()
                                    .is_none()
                            })
                })
            });
        let Some(index) = index else {
            break;
        };
        let (_, path) = roots.remove(index);
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn backup_path(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, destination)?;
    } else if metadata.is_dir() {
        LibrarySkillAcquisitionService::copy_tree_preserving_links(source, destination)?;
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn verify_backup(source: &Path, destination: &Path) -> Result<()> {
    let source_metadata = fs::symlink_metadata(source)?;
    let destination_metadata = fs::symlink_metadata(destination)?;
    if source_metadata.file_type().is_symlink() {
        if !destination_metadata.file_type().is_symlink()
            || fs::read_link(source)? != fs::read_link(destination)?
        {
            return Err(anyhow!("migration link backup verification failed"));
        }
    } else if source_metadata.is_dir() {
        let source_hash = LibrarySkillAcquisitionService::compute_library_hash(source)?;
        let backup_hash = LibrarySkillAcquisitionService::compute_library_hash(destination)?;
        if source_hash != backup_hash {
            return Err(anyhow!("migration content backup verification failed"));
        }
    } else if fs::read(source)? != fs::read(destination)? {
        return Err(anyhow!("migration file backup verification failed"));
    }
    Ok(())
}

fn validate_restore_safety(
    db: &Database,
    items: &[SkillsMigrationItemRecord],
    backup_root: &Path,
) -> Result<()> {
    for item in items {
        let Some(original) = item.source_location.as_deref().map(Path::new) else {
            continue;
        };
        let parked = retired_source_staging(item, original);
        match fs::symlink_metadata(&parked) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if item.action != "move_to_library" && item.action != "reuse_library" {
                    return Err(anyhow!(
                        "migration restore found unexpected retired content"
                    ));
                }
                let expected = item.expected_fingerprint.as_deref().unwrap_or_default();
                if !physical_identity_matches(&parked, expected) {
                    return Err(anyhow!("migration restore retired content drifted"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(anyhow!("migration restore retired content is invalid")),
            Err(error) => return Err(error.into()),
        }
        let backup = content_backup_source_path(backup_root, item.ordinal);
        if fs::symlink_metadata(&backup).is_err() {
            continue;
        }
        match item.action.as_str() {
            "reuse_library"
                if (item
                    .target_location
                    .as_deref()
                    .is_some_and(|target| Path::new(target) == original)
                    || original.parent().is_some_and(|parent| {
                        parent == LibrarySkillAcquisitionService::library_directory_path()
                    }))
                    && !physical_identity_matches(
                        original,
                        item.expected_fingerprint.as_deref().unwrap_or_default(),
                    ) =>
            {
                return Err(anyhow!("migration restore Library source drifted"))
            }
            "reuse_library"
                if item
                    .target_location
                    .as_deref()
                    .is_some_and(|target| Path::new(target) == original)
                    || original.parent().is_some_and(|parent| {
                        parent == LibrarySkillAcquisitionService::library_directory_path()
                    }) => {}
            "move_to_library" | "reuse_library" | "remove_legacy_codex_link" => {
                match fs::symlink_metadata(original) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(anyhow!(
                            "migration restore refused occupied source {}",
                            original.display()
                        ))
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            _ => {}
        }
    }

    for item in items.iter().rev() {
        let Some(target) = item.target_location.as_deref().map(Path::new) else {
            continue;
        };
        match item.action.as_str() {
            "create_global_deployment" => match fs::symlink_metadata(target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let directory = item
                        .directory
                        .as_deref()
                        .ok_or_else(|| anyhow!("deployment journal is missing its directory"))?;
                    let expected =
                        LibrarySkillAcquisitionService::library_directory_path().join(directory);
                    if fs::canonicalize(target).ok() != fs::canonicalize(expected).ok() {
                        return Err(anyhow!("restore refused a drifted deployment link"));
                    }
                    let Some(consumer) = parse_consumer(item.consumer.as_deref()) else {
                        return Err(anyhow!("deployment journal has an invalid consumer"));
                    };
                    let Some(skill) = db.get_library_skill_by_directory(directory)? else {
                        return Err(anyhow!("restore deployment Library row disappeared"));
                    };
                    if db
                        .get_skill_deployment(&skill.id, &DeploymentTarget::global(consumer))?
                        .is_none()
                    {
                        return Err(anyhow!("restore refused an unowned deployment link"));
                    }
                }
                Ok(_) => return Err(anyhow!("restore target is occupied by unmanaged content")),
                Err(error) => return Err(error.into()),
            },
            "move_to_library" => match fs::symlink_metadata(target) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let directory = item
                        .directory
                        .as_deref()
                        .ok_or_else(|| anyhow!("migration journal is missing its directory"))?;
                    let Some(skill) = db.get_library_skill_by_directory(directory)? else {
                        return Err(anyhow!("restore Library row disappeared"));
                    };
                    let hash = LibrarySkillAcquisitionService::compute_library_hash(target)?;
                    if skill.id != item.library_skill_id.as_deref().unwrap_or(&skill.id)
                        || hash != skill.content_hash
                    {
                        return Err(anyhow!("restore Library target drifted"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(anyhow!("restore Library target is unsafe to remove")),
                Err(error) => return Err(error.into()),
            },
            _ => {}
        }
    }
    Ok(())
}

fn restore_backup_path(
    backup: &Path,
    original: &Path,
    item: &SkillsMigrationItemRecord,
) -> Result<()> {
    match fs::symlink_metadata(original) {
        Ok(_) if item.action == "remove_legacy_codex_link" => {
            return Err(anyhow!("migration restore legacy path is occupied"));
        }
        Ok(_) if item.action == "move_to_library" => {
            return Err(anyhow!("migration restore retired source is occupied"));
        }
        Ok(metadata)
            if item.action == "reuse_library"
                && physical_identity_matches(
                    original,
                    item.expected_fingerprint.as_deref().unwrap_or_default(),
                ) =>
        {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(original)?;
            } else if metadata.file_type().is_symlink() || metadata.is_file() {
                fs::remove_file(original)?;
            } else {
                return Err(anyhow!("migration restore target is unsupported"));
            }
        }
        Ok(_) if item.action == "reuse_library" => {
            return Err(anyhow!("migration restore retired source is occupied"));
        }
        Ok(_) => return Err(anyhow!("migration restore target is occupied")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    backup_path(backup, original)
}

fn restore_remove_migration_outputs(
    db: &Database,
    items: &[SkillsMigrationItemRecord],
) -> Result<()> {
    for item in items.iter().rev() {
        let Some(target) = item.target_location.as_deref().map(Path::new) else {
            continue;
        };
        match item.action.as_str() {
            "create_global_deployment" => match fs::symlink_metadata(target) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let directory = item
                        .directory
                        .as_deref()
                        .ok_or_else(|| anyhow!("deployment journal is missing its directory"))?;
                    let expected = crate::config::get_app_config_dir()
                        .join("skills")
                        .join(directory);
                    if fs::canonicalize(target).ok() != fs::canonicalize(expected).ok() {
                        return Err(anyhow!("restore refused a drifted deployment link"));
                    }
                    let consumer = parse_consumer(item.consumer.as_deref())
                        .ok_or_else(|| anyhow!("deployment journal has an invalid consumer"))?;
                    let skill = db
                        .get_library_skill_by_directory(directory)?
                        .ok_or_else(|| anyhow!("deployment Library row disappeared"))?;
                    if db
                        .get_skill_deployment(&skill.id, &DeploymentTarget::global(consumer))?
                        .is_none()
                    {
                        return Err(anyhow!("restore refused an unowned deployment link"));
                    }
                    if item.expected_fingerprint.is_none() {
                        fs::remove_file(target)?
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(anyhow!("restore target is occupied by unmanaged content")),
                Err(error) => return Err(error.into()),
            },
            "move_to_library" => match fs::symlink_metadata(target) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let directory = item
                        .directory
                        .as_deref()
                        .ok_or_else(|| anyhow!("migration journal is missing its directory"))?;
                    let skill = db
                        .get_library_skill_by_directory(directory)?
                        .ok_or_else(|| anyhow!("migration Library row disappeared"))?;
                    let hash = LibrarySkillAcquisitionService::compute_library_hash(target)?;
                    if hash != skill.content_hash {
                        return Err(anyhow!("restore Library target drifted"));
                    }
                    fs::remove_dir_all(target)?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(anyhow!("restore Library target is unsafe to remove")),
                Err(error) => return Err(error.into()),
            },
            _ => {}
        }
    }
    Ok(())
}

fn restore_database_file(db: &Database, backup: &Path) -> Result<()> {
    db.restore_database_snapshot_file(backup)?;
    Ok(())
}

#[cfg(debug_assertions)]
static FAIL_BEFORE_ACTION: OnceLock<Mutex<Option<String>>> = OnceLock::new();
#[cfg(debug_assertions)]
static FAIL_COMPENSATION_ACTION: OnceLock<Mutex<Option<String>>> = OnceLock::new();
#[cfg(debug_assertions)]
static REMOVE_SOURCE_BEFORE_BACKUP: OnceLock<Mutex<Option<String>>> = OnceLock::new();
#[cfg(debug_assertions)]
static FAIL_JOURNAL_COMPLETION: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(debug_assertions)]
fn consume_action_failpoint(cell: &OnceLock<Mutex<Option<String>>>, action: &str) -> bool {
    cell.get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut selected| {
            if selected.as_deref() == Some(action) {
                selected.take()
            } else {
                None
            }
        })
        .is_some()
}

#[cfg(debug_assertions)]
fn should_remove_source_before_backup(action: &str) -> bool {
    consume_action_failpoint(&REMOVE_SOURCE_BEFORE_BACKUP, action)
}

#[cfg(debug_assertions)]
fn should_fail_journal_completion(action: &str) -> bool {
    consume_action_failpoint(&FAIL_JOURNAL_COMPLETION, action)
}

#[cfg(debug_assertions)]
fn remove_path_for_test(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn should_interrupt_before(action: &str) -> bool {
    FAIL_BEFORE_ACTION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut selected| {
            if selected.as_deref() == Some(action) {
                selected.take()
            } else {
                None
            }
        })
        .is_some()
}

#[cfg(debug_assertions)]
fn should_fail_compensation(action: &str) -> bool {
    FAIL_COMPENSATION_ACTION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut selected| {
            if selected.as_deref() == Some(action) {
                selected.take()
            } else {
                None
            }
        })
        .is_some()
}

#[cfg(debug_assertions)]
impl SkillsMigrationExecutionService {
    #[doc(hidden)]
    pub fn interrupt_before_action_for_test(action: Option<SkillsMigrationAction>) {
        if let Ok(mut selected) = FAIL_BEFORE_ACTION.get_or_init(|| Mutex::new(None)).lock() {
            *selected = action.map(action_name).map(str::to_string);
        }
    }

    #[doc(hidden)]
    pub fn fail_compensation_for_action_for_test(action: Option<SkillsMigrationAction>) {
        if let Ok(mut selected) = FAIL_COMPENSATION_ACTION
            .get_or_init(|| Mutex::new(None))
            .lock()
        {
            *selected = action.map(action_name).map(str::to_string);
        }
    }

    #[doc(hidden)]
    pub fn remove_source_before_backup_for_test(action: Option<SkillsMigrationAction>) {
        if let Ok(mut selected) = REMOVE_SOURCE_BEFORE_BACKUP
            .get_or_init(|| Mutex::new(None))
            .lock()
        {
            *selected = action.map(action_name).map(str::to_string);
        }
    }

    #[doc(hidden)]
    pub fn fail_journal_completion_for_test(action: Option<SkillsMigrationAction>) {
        if let Ok(mut selected) = FAIL_JOURNAL_COMPLETION
            .get_or_init(|| Mutex::new(None))
            .lock()
        {
            *selected = action.map(action_name).map(str::to_string);
        }
    }

    #[doc(hidden)]
    pub fn interrupt_after_database_restore_for_test(enabled: bool) {
        if let Ok(mut selected) = INTERRUPT_AFTER_DATABASE_RESTORE
            .get_or_init(|| Mutex::new(false))
            .lock()
        {
            *selected = enabled;
        }
    }

    #[doc(hidden)]
    pub fn interrupt_after_source_retire_for_test(enabled: bool) {
        if let Ok(mut selected) = INTERRUPT_AFTER_SOURCE_RETIRE
            .get_or_init(|| Mutex::new(false))
            .lock()
        {
            *selected = enabled;
        }
    }
}

#[cfg(debug_assertions)]
fn should_interrupt_after_database_restore() -> bool {
    INTERRUPT_AFTER_DATABASE_RESTORE
        .get_or_init(|| Mutex::new(false))
        .lock()
        .map(|mut enabled| std::mem::take(&mut *enabled))
        .unwrap_or(false)
}

#[cfg(debug_assertions)]
fn should_interrupt_after_source_retire() -> bool {
    INTERRUPT_AFTER_SOURCE_RETIRE
        .get_or_init(|| Mutex::new(false))
        .lock()
        .map(|mut enabled| std::mem::take(&mut *enabled))
        .unwrap_or(false)
}
