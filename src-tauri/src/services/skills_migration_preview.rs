//! Read-only preview for the guided macOS Skills migration.
//!
//! This module deliberately owns no mutation seam. It observes legacy database
//! evidence and fixed, application-owned roots, then returns a deterministic
//! plan for display. A later migration issue owns the journaled cutover.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::app_config::{AppType, InstalledSkill};
use crate::config::{get_app_config_dir, get_home_dir};
use crate::database::Database;
use crate::services::skill::{LibrarySkill, SkillStorageLocation};
use crate::services::skill_deployment::DeploymentConsumer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationStatus {
    NotRequired,
    DecisionNeeded,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationPageMode {
    Writable,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationInventoryKind {
    ManagedLibrary,
    LegacySkill,
    UnmanagedContent,
    TargetConflict,
    LegacyCodexEntry,
    ScanError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationInventoryState {
    Present,
    Missing,
    RealDirectory,
    ManagedLink,
    ForeignLink,
    BrokenLink,
    Occupied,
    Unreadable,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationInventoryItem {
    pub kind: SkillsMigrationInventoryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<DeploymentConsumer>,
    /// Display-only observation. No migration command accepts this value back.
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub state: SkillsMigrationInventoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationDisposition {
    Perform,
    Preserve,
    /// The item is outside CC Switch's managed consumer set.  Apply is
    /// allowed to proceed only after the user explicitly consents to leave
    /// the observed files untouched.
    PreserveWithConsent,
    UserResolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationAction {
    MoveToLibrary,
    ReuseLibrary,
    CreateGlobalDeployment,
    RemoveLegacyCodexLink,
    PreserveContent,
    PreserveUnsupportedConsumerFiles,
    ResolveConflict,
    RepairPreflight,
    Finalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMigrationReason {
    ProvenManaged,
    AlreadyInLibrary,
    LegacyEnabled,
    ProvenCcSwitchLink,
    Unmanaged,
    UnsupportedConsumerEnabled,
    ForeignOrAmbiguous,
    ContentConflict,
    MissingSource,
    InvalidLegacyState,
    Unreadable,
    MigrationFinalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationPlanItem {
    pub disposition: SkillsMigrationDisposition,
    pub action: SkillsMigrationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<DeploymentConsumer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_location: Option<String>,
    pub reason: SkillsMigrationReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_consumers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationRevealIntent {
    pub observation_token: String,
    pub plan_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationBackupPlan {
    pub required: bool,
    pub ready: bool,
    pub recovery_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_path: Option<String>,
    pub content_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsMigrationPreflight {
    pub status: SkillsMigrationStatus,
    pub observation_token: String,
    pub page_mode: SkillsMigrationPageMode,
    pub inventory: Vec<SkillsMigrationInventoryItem>,
    pub plan: Vec<SkillsMigrationPlanItem>,
    pub backup: SkillsMigrationBackupPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::services::skills_migration::SkillsMigrationExecutionResult>,
}

/// Re-inspect the fixed migration roots for a caller that already holds every
/// migration mutation lock. Keeping this crate-visible avoids any second plan
/// compiler with subtly different ownership rules.
pub(crate) fn inspect_locked(db: Arc<Database>) -> Result<SkillsMigrationPreflight> {
    SkillsMigrationPreviewService::new(db).inspect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacySnapshotRow {
    directory: String,
    app_type: String,
    /// Historical snapshots contained only installed rows and omitted this
    /// field, so omission means true rather than bool's normal false default.
    #[serde(default = "legacy_installed_default")]
    installed: bool,
}

fn legacy_installed_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
struct Observation {
    location: String,
    fingerprint: String,
}

pub struct SkillsMigrationPreviewService {
    db: Arc<Database>,
}

impl SkillsMigrationPreviewService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn inspect(&self) -> Result<SkillsMigrationPreflight> {
        let execution = self
            .db
            .get_active_skills_migration_run()?
            .and_then(|run| {
                let outcome = match run.state.as_str() {
                    "prepared" | "running" => {
                        crate::services::skills_migration::SkillsMigrationExecutionOutcome::Resumable
                    }
                    "blocked" => {
                        crate::services::skills_migration::SkillsMigrationExecutionOutcome::Blocked
                    }
                    "recovery_required" => crate::services::skills_migration::SkillsMigrationExecutionOutcome::RecoveryRequired,
                    _ => return None,
                };
                crate::services::skills_migration::SkillsMigrationExecutionService::new(
                    self.db.clone(),
                )
                .inspect_run(&run, outcome)
                .ok()
            });
        let pending = self.db.get_setting("skills_ssot_migration_pending")?;
        let snapshot_raw = self.db.get_setting("skills_ssot_migration_snapshot")?;
        let legacy_skills = self.db.get_all_installed_skills()?;
        let library_skills = self.db.list_library_skills()?;

        let mut blocked = false;
        let mut inventory = Vec::new();
        let mut plan = Vec::new();
        let snapshot = match snapshot_raw.as_deref().filter(|raw| !raw.trim().is_empty()) {
            Some(raw) => match serde_json::from_str::<Vec<LegacySnapshotRow>>(raw) {
                Ok(rows) => rows,
                Err(_) => {
                    blocked = true;
                    inventory.push(scan_error(
                        get_app_config_dir().join("cc-switch.db"),
                        SkillsMigrationInventoryState::Invalid,
                    ));
                    plan.push(repair_plan(SkillsMigrationReason::InvalidLegacyState));
                    Vec::new()
                }
            },
            None => Vec::new(),
        };

        let decision_exists = pending
            .as_deref()
            .is_some_and(|flag| flag == "true" || flag == "1")
            || !snapshot.is_empty()
            || !legacy_skills.is_empty()
            || blocked;

        let roots = fixed_roots();
        let database_path = get_app_config_dir().join("cc-switch.db");
        if !decision_exists {
            let backup = SkillsMigrationBackupPlan {
                required: false,
                ready: true,
                recovery_available: recovery_exists(),
                database_path: database_path.exists().then(|| display_path(&database_path)),
                content_paths: Vec::new(),
            };
            let token = observation_token(
                pending.as_deref(),
                snapshot_raw.as_deref(),
                &legacy_skills.values().cloned().collect::<Vec<_>>(),
                &library_skills,
                &inventory,
                &plan,
                &backup,
                &root_observations(&roots),
            )?;
            return Ok(SkillsMigrationPreflight {
                status: SkillsMigrationStatus::NotRequired,
                observation_token: token,
                page_mode: SkillsMigrationPageMode::Writable,
                inventory,
                plan,
                backup,
                execution,
            });
        }

        let mut claimed = BTreeSet::new();
        let library_by_directory: BTreeMap<_, _> = library_skills
            .iter()
            .map(|skill| (skill.directory.as_str(), skill))
            .collect();
        let managed_paths =
            managed_paths(&library_skills, legacy_skills.values(), &roots.legacy_ssot);

        for skill in &library_skills {
            let path = roots.library.join(&skill.directory);
            let state = classify_library_path(skill, &path);
            if state != SkillsMigrationInventoryState::Present {
                blocked = true;
            }
            claimed.insert(path.clone());
            inventory.push(SkillsMigrationInventoryItem {
                kind: SkillsMigrationInventoryKind::ManagedLibrary,
                directory: Some(skill.directory.clone()),
                consumer: None,
                location: display_path(&path),
                managed_skill_id: Some(skill.id.clone()),
                enabled: None,
                state,
            });
        }

        let mut evidence = snapshot;
        evidence.sort_by(|left, right| {
            left.directory
                .cmp(&right.directory)
                .then(left.app_type.cmp(&right.app_type))
                .then(left.installed.cmp(&right.installed))
        });
        evidence.dedup_by(|left, right| {
            left.directory == right.directory
                && left.app_type == right.app_type
                && left.installed == right.installed
        });
        for row in evidence {
            let Some(consumer) = parse_legacy_consumer(&row.app_type) else {
                blocked = true;
                inventory.push(SkillsMigrationInventoryItem {
                    kind: SkillsMigrationInventoryKind::LegacySkill,
                    directory: Some(row.directory.clone()),
                    consumer: None,
                    location: format!("legacy-consumer:{}", row.app_type),
                    managed_skill_id: None,
                    enabled: Some(row.installed),
                    state: SkillsMigrationInventoryState::Invalid,
                });
                plan.push(SkillsMigrationPlanItem {
                    disposition: SkillsMigrationDisposition::UserResolve,
                    action: SkillsMigrationAction::RepairPreflight,
                    directory: Some(row.directory),
                    consumer: None,
                    from_location: None,
                    to_location: None,
                    reason: SkillsMigrationReason::InvalidLegacyState,
                    unsupported_consumers: Vec::new(),
                });
                continue;
            };
            if !valid_directory(&row.directory) {
                blocked = true;
                inventory.push(SkillsMigrationInventoryItem {
                    kind: SkillsMigrationInventoryKind::LegacySkill,
                    directory: Some(row.directory.clone()),
                    consumer: Some(consumer),
                    location: display_path(legacy_root(consumer)),
                    managed_skill_id: None,
                    enabled: Some(row.installed),
                    state: SkillsMigrationInventoryState::Invalid,
                });
                plan.push(SkillsMigrationPlanItem {
                    disposition: SkillsMigrationDisposition::UserResolve,
                    action: SkillsMigrationAction::RepairPreflight,
                    directory: Some(row.directory),
                    consumer: Some(consumer),
                    from_location: None,
                    to_location: None,
                    reason: SkillsMigrationReason::InvalidLegacyState,
                    unsupported_consumers: Vec::new(),
                });
                continue;
            }
            let path = legacy_root(consumer).join(&row.directory);
            add_legacy_evidence(
                &mut inventory,
                &mut plan,
                &mut claimed,
                &mut blocked,
                &managed_paths,
                library_by_directory.get(row.directory.as_str()).copied(),
                &row.directory,
                consumer,
                row.installed,
                row.installed,
                None,
                path,
            );
        }

        let mut current = legacy_skills.values().collect::<Vec<_>>();
        current.sort_by(|left, right| {
            left.directory
                .cmp(&right.directory)
                .then(left.id.cmp(&right.id))
        });
        for skill in current {
            add_current_legacy_skill(
                &mut inventory,
                &mut plan,
                &mut claimed,
                &mut blocked,
                &roots,
                &managed_paths,
                &library_by_directory,
                skill,
            );
        }
        reconcile_duplicate_library_actions(&mut inventory, &mut plan);
        suppress_deployments_with_content_conflicts(&mut plan);
        if plan.iter().any(plan_item_requires_manual_resolution) {
            blocked = true;
        }

        for (root, codex_legacy) in &roots.scan_roots {
            scan_unclaimed(
                root,
                *codex_legacy,
                &managed_paths,
                &mut claimed,
                &mut inventory,
                &mut plan,
                &mut blocked,
            );
        }

        sort_inventory(&mut inventory);
        sort_plan(&mut plan);
        plan.dedup();
        let mut backup_sources = plan
            .iter()
            .filter(|item| {
                matches!(
                    item.action,
                    SkillsMigrationAction::MoveToLibrary
                        | SkillsMigrationAction::ReuseLibrary
                        | SkillsMigrationAction::RemoveLegacyCodexLink
                )
            })
            .filter_map(|item| item.from_location.clone().map(|path| (item.action, path)))
            .collect::<Vec<_>>();
        backup_sources.extend(
            inventory
                .iter()
                .filter(|item| {
                    item.kind == SkillsMigrationInventoryKind::LegacySkill
                        && item.enabled == Some(true)
                        && item.state == SkillsMigrationInventoryState::RealDirectory
                })
                .map(|item| (SkillsMigrationAction::MoveToLibrary, item.location.clone())),
        );
        backup_sources.sort_by(|left, right| left.1.cmp(&right.1));
        backup_sources.dedup_by(|left, right| left.1 == right.1);
        let mut content_paths = backup_sources
            .iter()
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        content_paths.sort();
        content_paths.dedup();
        let content_ready = backup_sources
            .iter()
            .all(|(action, path)| backup_source_ready(*action, Path::new(path), &managed_paths));
        let database_ready = fs::symlink_metadata(&database_path)
            .map(|metadata| metadata.file_type().is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
            && fs::File::open(&database_path).is_ok();
        if !database_ready {
            blocked = true;
            inventory.push(scan_error(
                database_path.clone(),
                SkillsMigrationInventoryState::Missing,
            ));
            plan.push(repair_plan(SkillsMigrationReason::MissingSource));
            sort_inventory(&mut inventory);
            sort_plan(&mut plan);
            plan.dedup();
        }
        let backup = SkillsMigrationBackupPlan {
            required: true,
            // `ready` describes backup prerequisites, not the existence of a
            // completed backup. `recovery_available` is the latter fact.
            ready: !blocked && database_ready && content_ready,
            recovery_available: recovery_exists(),
            database_path: database_ready.then(|| display_path(&database_path)),
            content_paths,
        };
        let observations = root_observations(&roots);
        let token = observation_token(
            pending.as_deref(),
            snapshot_raw.as_deref(),
            &legacy_skills.values().cloned().collect::<Vec<_>>(),
            &library_skills,
            &inventory,
            &plan,
            &backup,
            &observations,
        )?;
        Ok(SkillsMigrationPreflight {
            status: if blocked {
                SkillsMigrationStatus::Blocked
            } else {
                SkillsMigrationStatus::DecisionNeeded
            },
            observation_token: token,
            page_mode: SkillsMigrationPageMode::ReadOnly,
            inventory,
            plan,
            backup,
            execution,
        })
    }

    /// Resolve a display-only plan index back to a trusted directory after
    /// revalidating the opaque observation token. The renderer never supplies
    /// a filesystem path to this boundary.
    pub fn resolve_reveal_directory(&self, intent: SkillsMigrationRevealIntent) -> Result<PathBuf> {
        let preflight = self.inspect()?;
        if preflight.observation_token != intent.observation_token {
            anyhow::bail!("Skills migration preview changed; recheck before revealing a path");
        }
        let item = preflight
            .plan
            .get(intent.plan_index)
            .ok_or_else(|| anyhow::anyhow!("Skills migration plan item no longer exists"))?;
        let location = item
            .from_location
            .as_deref()
            .or(item.to_location.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Skills migration plan item has no revealable path"))?;
        let path = Path::new(location);
        let directory = path.parent().ok_or_else(|| {
            anyhow::anyhow!("Skills migration plan item has no containing folder")
        })?;
        if !directory.is_dir() {
            anyhow::bail!("Skills migration plan item containing folder is unavailable");
        }
        Ok(directory.to_path_buf())
    }
}

struct FixedRoots {
    library: PathBuf,
    codex: PathBuf,
    legacy_ssot: PathBuf,
    scan_roots: Vec<(PathBuf, bool)>,
}

fn fixed_roots() -> FixedRoots {
    let home = get_home_dir();
    let claude = crate::services::skill::SkillService::get_app_skills_dir(&AppType::Claude)
        .unwrap_or_else(|_| home.join(".claude/skills"));
    let codex_legacy = crate::services::skill::SkillService::get_app_skills_dir(&AppType::Codex)
        .unwrap_or_else(|_| home.join(".codex/skills"));
    let library = get_app_config_dir().join("skills");
    let codex = home.join(".agents/skills");
    let legacy_ssot = match crate::settings::get_skill_storage_location() {
        SkillStorageLocation::CcSwitch => library.clone(),
        SkillStorageLocation::Unified => codex.clone(),
    };
    let mut scan_roots = vec![
        (library.clone(), false),
        (claude.clone(), false),
        (codex_legacy.clone(), true),
        (codex.clone(), false),
        (home.join(".claude/skills"), false),
        (home.join(".codex/skills"), true),
    ];
    scan_roots.sort();
    scan_roots.dedup();
    FixedRoots {
        library,
        codex,
        legacy_ssot,
        scan_roots,
    }
}

fn legacy_root(consumer: DeploymentConsumer) -> PathBuf {
    match consumer {
        DeploymentConsumer::Claude => {
            crate::services::skill::SkillService::get_app_skills_dir(&AppType::Claude)
                .unwrap_or_else(|_| get_home_dir().join(".claude/skills"))
        }
        DeploymentConsumer::Codex => {
            crate::services::skill::SkillService::get_app_skills_dir(&AppType::Codex)
                .unwrap_or_else(|_| get_home_dir().join(".codex/skills"))
        }
    }
}

fn deployment_root(consumer: DeploymentConsumer) -> PathBuf {
    match consumer {
        DeploymentConsumer::Claude => get_home_dir().join(".claude/skills"),
        DeploymentConsumer::Codex => get_home_dir().join(".agents/skills"),
    }
}

fn parse_legacy_consumer(value: &str) -> Option<DeploymentConsumer> {
    match value {
        "claude" => Some(DeploymentConsumer::Claude),
        "codex" => Some(DeploymentConsumer::Codex),
        _ => None,
    }
}

fn valid_directory(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !Path::new(value).is_absolute()
        && Path::new(value).components().count() == 1
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}

#[allow(clippy::too_many_arguments)]
fn add_legacy_evidence(
    inventory: &mut Vec<SkillsMigrationInventoryItem>,
    plan: &mut Vec<SkillsMigrationPlanItem>,
    claimed: &mut BTreeSet<PathBuf>,
    blocked: &mut bool,
    managed_paths: &BTreeSet<PathBuf>,
    library: Option<&LibrarySkill>,
    directory: &str,
    consumer: DeploymentConsumer,
    enabled: bool,
    manage_content: bool,
    managed_skill_id: Option<String>,
    path: PathBuf,
) {
    if inventory.iter().any(|item| {
        item.directory.as_deref() == Some(directory)
            && item.consumer == Some(consumer)
            && item.location == display_path(&path)
            && item.enabled == Some(enabled)
    }) {
        return;
    }
    claimed.insert(path.clone());
    let state = classify_path(&path, managed_paths);
    inventory.push(SkillsMigrationInventoryItem {
        kind: SkillsMigrationInventoryKind::LegacySkill,
        directory: Some(directory.to_string()),
        consumer: Some(consumer),
        location: display_path(&path),
        managed_skill_id,
        enabled: Some(enabled),
        state,
    });
    if !enabled && !manage_content {
        if state != SkillsMigrationInventoryState::Missing {
            plan.push(SkillsMigrationPlanItem {
                disposition: SkillsMigrationDisposition::Preserve,
                action: SkillsMigrationAction::PreserveContent,
                directory: Some(directory.to_string()),
                consumer: Some(consumer),
                from_location: Some(display_path(&path)),
                to_location: None,
                reason: SkillsMigrationReason::Unmanaged,
                unsupported_consumers: Vec::new(),
            });
        }
        return;
    }
    match state {
        SkillsMigrationInventoryState::RealDirectory => {
            let observed = match crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(&path) {
                Ok(observed) => observed,
                Err(_) => {
                    *blocked = true;
                    if let Some(item) = inventory.iter_mut().rev().find(|item| {
                        item.directory.as_deref() == Some(directory)
                            && item.consumer == Some(consumer)
                            && item.location == display_path(&path)
                    }) {
                        item.state = SkillsMigrationInventoryState::Invalid;
                    }
                    plan.push(resolve_plan(
                        directory,
                        Some(consumer),
                        &path,
                        SkillsMigrationReason::InvalidLegacyState,
                    ));
                    return;
                }
            };
            let (action, reason, to_location) = match library {
                Some(library) if library.content_hash == observed.content_hash => (
                    SkillsMigrationAction::ReuseLibrary,
                    SkillsMigrationReason::AlreadyInLibrary,
                    Some(display_path(
                        get_app_config_dir().join("skills").join(directory),
                    )),
                ),
                Some(_) => {
                    inventory.push(SkillsMigrationInventoryItem {
                        kind: SkillsMigrationInventoryKind::TargetConflict,
                        directory: Some(directory.to_string()),
                        consumer: None,
                        location: display_path(get_app_config_dir().join("skills").join(directory)),
                        managed_skill_id: library.map(|skill| skill.id.clone()),
                        enabled: Some(enabled),
                        state: SkillsMigrationInventoryState::Occupied,
                    });
                    plan.push(resolve_plan(
                        directory,
                        Some(consumer),
                        &path,
                        SkillsMigrationReason::ContentConflict,
                    ));
                    return;
                }
                None => {
                    let destination = get_app_config_dir().join("skills").join(directory);
                    if path == destination {
                        (
                            SkillsMigrationAction::ReuseLibrary,
                            SkillsMigrationReason::AlreadyInLibrary,
                            Some(display_path(destination)),
                        )
                    } else {
                        (
                            SkillsMigrationAction::MoveToLibrary,
                            SkillsMigrationReason::ProvenManaged,
                            Some(display_path(destination)),
                        )
                    }
                }
            };
            plan.push(SkillsMigrationPlanItem {
                disposition: SkillsMigrationDisposition::Perform,
                action,
                directory: Some(directory.to_string()),
                consumer: None,
                from_location: Some(display_path(&path)),
                to_location,
                reason,
                unsupported_consumers: Vec::new(),
            });
            if enabled {
                plan.push(deployment_plan(directory, consumer, None));
            }
        }
        SkillsMigrationInventoryState::ManagedLink => {
            let official_target = deployment_root(consumer).join(directory);
            let is_official_target = path == official_target;
            if consumer == DeploymentConsumer::Codex && !is_official_target {
                plan.push(SkillsMigrationPlanItem {
                    disposition: SkillsMigrationDisposition::Perform,
                    action: SkillsMigrationAction::RemoveLegacyCodexLink,
                    directory: Some(directory.to_string()),
                    consumer: Some(consumer),
                    from_location: Some(display_path(&path)),
                    to_location: None,
                    reason: SkillsMigrationReason::ProvenCcSwitchLink,
                    unsupported_consumers: Vec::new(),
                });
            }
            if enabled {
                let proven_official_link = is_official_target.then_some(path.as_path());
                plan.push(deployment_plan(directory, consumer, proven_official_link));
            }
        }
        SkillsMigrationInventoryState::Missing => {
            *blocked = true;
            plan.push(resolve_plan(
                directory,
                Some(consumer),
                &path,
                SkillsMigrationReason::MissingSource,
            ));
        }
        SkillsMigrationInventoryState::Unreadable => {
            *blocked = true;
            plan.push(resolve_plan(
                directory,
                Some(consumer),
                &path,
                SkillsMigrationReason::Unreadable,
            ));
        }
        _ => plan.push(resolve_plan(
            directory,
            Some(consumer),
            &path,
            SkillsMigrationReason::ForeignOrAmbiguous,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn add_current_legacy_skill(
    inventory: &mut Vec<SkillsMigrationInventoryItem>,
    plan: &mut Vec<SkillsMigrationPlanItem>,
    claimed: &mut BTreeSet<PathBuf>,
    blocked: &mut bool,
    roots: &FixedRoots,
    managed_paths: &BTreeSet<PathBuf>,
    library_by_directory: &BTreeMap<&str, &LibrarySkill>,
    skill: &InstalledSkill,
) {
    if !valid_directory(&skill.directory) {
        *blocked = true;
        inventory.push(SkillsMigrationInventoryItem {
            kind: SkillsMigrationInventoryKind::LegacySkill,
            directory: Some(skill.directory.clone()),
            consumer: None,
            location: display_path(&roots.library),
            managed_skill_id: Some(skill.id.clone()),
            enabled: None,
            state: SkillsMigrationInventoryState::Invalid,
        });
        plan.push(repair_plan(SkillsMigrationReason::InvalidLegacyState));
        return;
    }
    let library = library_by_directory.get(skill.directory.as_str()).copied();
    let source = roots.legacy_ssot.join(&skill.directory);
    let alternate_root = if roots.legacy_ssot == roots.library {
        &roots.codex
    } else {
        &roots.library
    };
    let alternate = alternate_root.join(&skill.directory);
    if is_real_directory(&source) && is_real_directory(&alternate) {
        let source_hash =
            crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(
                &source,
            )
            .map(|inspection| inspection.content_hash);
        let alternate_hash =
            crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(
                &alternate,
            )
            .map(|inspection| inspection.content_hash);
        if !matches!((&source_hash, &alternate_hash), (Ok(left), Ok(right)) if left == right) {
            inventory.push(SkillsMigrationInventoryItem {
                kind: SkillsMigrationInventoryKind::TargetConflict,
                directory: Some(skill.directory.clone()),
                consumer: None,
                location: display_path(&alternate),
                managed_skill_id: Some(skill.id.clone()),
                enabled: None,
                state: SkillsMigrationInventoryState::Occupied,
            });
            plan.push(resolve_plan(
                &skill.directory,
                None,
                &alternate,
                SkillsMigrationReason::ContentConflict,
            ));
            return;
        }
    }
    for (consumer, enabled) in [
        (DeploymentConsumer::Claude, skill.apps.claude),
        (DeploymentConsumer::Codex, skill.apps.codex),
    ] {
        add_legacy_evidence(
            inventory,
            plan,
            claimed,
            blocked,
            managed_paths,
            library,
            &skill.directory,
            consumer,
            enabled,
            true,
            Some(skill.id.clone()),
            source.clone(),
        );
        if enabled {
            let official_target = deployment_root(consumer).join(&skill.directory);
            if is_exact_link_to(&official_target, &source) {
                if let Some(deployment) = plan.iter_mut().rev().find(|item| {
                    item.action == SkillsMigrationAction::CreateGlobalDeployment
                        && item.directory.as_deref() == Some(skill.directory.as_str())
                        && item.consumer == Some(consumer)
                }) {
                    deployment.from_location = Some(display_path(official_target));
                }
            }
        }
    }
    let unsupported_consumers = [
        ("gemini", skill.apps.gemini),
        ("grokbuild", skill.apps.grokbuild),
        ("opencode", skill.apps.opencode),
        ("hermes", skill.apps.hermes),
    ]
    .into_iter()
    .filter_map(|(consumer, enabled)| enabled.then_some(consumer.to_string()))
    .collect::<Vec<_>>();
    if !unsupported_consumers.is_empty() {
        inventory.push(SkillsMigrationInventoryItem {
            kind: SkillsMigrationInventoryKind::LegacySkill,
            directory: Some(skill.directory.clone()),
            consumer: None,
            location: display_path(&source),
            managed_skill_id: Some(skill.id.clone()),
            enabled: Some(true),
            state: classify_path(&source, managed_paths),
        });
        plan.push(SkillsMigrationPlanItem {
            disposition: SkillsMigrationDisposition::PreserveWithConsent,
            action: SkillsMigrationAction::PreserveUnsupportedConsumerFiles,
            directory: Some(skill.directory.clone()),
            consumer: None,
            from_location: Some(display_path(&source)),
            to_location: None,
            reason: SkillsMigrationReason::UnsupportedConsumerEnabled,
            unsupported_consumers,
        });
    }
}

fn deployment_plan(
    directory: &str,
    consumer: DeploymentConsumer,
    proven_official_link: Option<&Path>,
) -> SkillsMigrationPlanItem {
    SkillsMigrationPlanItem {
        disposition: SkillsMigrationDisposition::Perform,
        action: SkillsMigrationAction::CreateGlobalDeployment,
        directory: Some(directory.to_string()),
        consumer: Some(consumer),
        from_location: proven_official_link.map(display_path),
        to_location: Some(display_path(deployment_root(consumer).join(directory))),
        reason: SkillsMigrationReason::LegacyEnabled,
        unsupported_consumers: Vec::new(),
    }
}

fn is_exact_link_to(link: &Path, expected: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(link) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(raw_target) = fs::read_link(link) else {
        return false;
    };
    let resolved = if raw_target.is_absolute() {
        raw_target
    } else {
        let Some(parent) = link.parent() else {
            return false;
        };
        parent.join(raw_target)
    };
    resolved == expected
        || matches!(
            (fs::canonicalize(resolved), fs::canonicalize(expected)),
            (Ok(left), Ok(right)) if left == right
        )
}

fn reconcile_duplicate_library_actions(
    inventory: &mut Vec<SkillsMigrationInventoryItem>,
    plan: &mut Vec<SkillsMigrationPlanItem>,
) {
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in plan.iter().filter(|item| {
        matches!(
            item.action,
            SkillsMigrationAction::MoveToLibrary | SkillsMigrationAction::ReuseLibrary
        )
    }) {
        if let (Some(directory), Some(source)) = (&item.directory, &item.from_location) {
            sources
                .entry(directory.clone())
                .or_default()
                .push(source.clone());
        }
    }
    for (directory, mut paths) in sources {
        paths.sort();
        paths.dedup();
        if paths.len() < 2 {
            continue;
        }
        let hashes = paths
            .iter()
            .map(|path| {
                crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(
                    Path::new(path),
                )
                .map(|inspection| inspection.content_hash)
            })
            .collect::<Result<Vec<_>>>();
        let identical = hashes
            .ok()
            .is_some_and(|hashes| hashes.windows(2).all(|pair| pair[0] == pair[1]));
        if identical {
            let library_destination =
                display_path(get_app_config_dir().join("skills").join(directory.as_str()));
            let library_is_existing_source = paths.contains(&library_destination);
            let mut kept = false;
            for item in plan.iter_mut().filter(|item| {
                matches!(
                    item.action,
                    SkillsMigrationAction::MoveToLibrary | SkillsMigrationAction::ReuseLibrary
                ) && item.directory.as_deref() == Some(directory.as_str())
            }) {
                if library_is_existing_source {
                    item.action = SkillsMigrationAction::ReuseLibrary;
                    item.to_location = Some(library_destination.clone());
                    item.reason = SkillsMigrationReason::AlreadyInLibrary;
                } else if !kept {
                    kept = true;
                } else {
                    // The first item admits/reuses the Library identity. Every
                    // other proven identical source is still journaled so the
                    // executor retires the duplicate and restore can recreate it.
                    item.action = SkillsMigrationAction::ReuseLibrary;
                    item.to_location = Some(library_destination.clone());
                    item.reason = SkillsMigrationReason::AlreadyInLibrary;
                }
            }
        } else {
            plan.retain(|item| {
                !matches!(
                    item.action,
                    SkillsMigrationAction::MoveToLibrary | SkillsMigrationAction::ReuseLibrary
                ) || item.directory.as_deref() != Some(directory.as_str())
            });
            plan.retain(|item| {
                item.action != SkillsMigrationAction::CreateGlobalDeployment
                    || item.directory.as_deref() != Some(directory.as_str())
            });
            let destination = get_app_config_dir().join("skills").join(&directory);
            inventory.push(SkillsMigrationInventoryItem {
                kind: SkillsMigrationInventoryKind::TargetConflict,
                directory: Some(directory.clone()),
                consumer: None,
                location: display_path(&destination),
                managed_skill_id: None,
                enabled: Some(true),
                state: SkillsMigrationInventoryState::Occupied,
            });
            plan.push(SkillsMigrationPlanItem {
                disposition: SkillsMigrationDisposition::UserResolve,
                action: SkillsMigrationAction::ResolveConflict,
                directory: Some(directory),
                consumer: None,
                from_location: None,
                to_location: Some(display_path(destination)),
                reason: SkillsMigrationReason::ContentConflict,
                unsupported_consumers: Vec::new(),
            });
        }
    }
}

fn suppress_deployments_with_content_conflicts(plan: &mut Vec<SkillsMigrationPlanItem>) {
    let conflicts = plan
        .iter()
        .filter(|item| {
            item.disposition == SkillsMigrationDisposition::UserResolve
                && item.reason == SkillsMigrationReason::ContentConflict
        })
        .filter_map(|item| item.directory.clone())
        .collect::<BTreeSet<_>>();
    plan.retain(|item| {
        item.action != SkillsMigrationAction::CreateGlobalDeployment
            || item
                .directory
                .as_ref()
                .is_none_or(|directory| !conflicts.contains(directory))
    });
}

fn resolve_plan(
    directory: &str,
    consumer: Option<DeploymentConsumer>,
    path: &Path,
    reason: SkillsMigrationReason,
) -> SkillsMigrationPlanItem {
    SkillsMigrationPlanItem {
        disposition: SkillsMigrationDisposition::UserResolve,
        action: SkillsMigrationAction::ResolveConflict,
        directory: Some(directory.to_string()),
        consumer,
        from_location: Some(display_path(path)),
        to_location: None,
        reason,
        unsupported_consumers: Vec::new(),
    }
}

fn repair_plan(reason: SkillsMigrationReason) -> SkillsMigrationPlanItem {
    SkillsMigrationPlanItem {
        disposition: SkillsMigrationDisposition::UserResolve,
        action: SkillsMigrationAction::RepairPreflight,
        directory: None,
        consumer: None,
        from_location: None,
        to_location: None,
        reason,
        unsupported_consumers: Vec::new(),
    }
}

fn scan_error(path: PathBuf, state: SkillsMigrationInventoryState) -> SkillsMigrationInventoryItem {
    SkillsMigrationInventoryItem {
        kind: SkillsMigrationInventoryKind::ScanError,
        directory: None,
        consumer: None,
        location: display_path(path),
        managed_skill_id: None,
        enabled: None,
        state,
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_unclaimed(
    root: &Path,
    codex_legacy: bool,
    managed_paths: &BTreeSet<PathBuf>,
    claimed: &mut BTreeSet<PathBuf>,
    inventory: &mut Vec<SkillsMigrationInventoryItem>,
    plan: &mut Vec<SkillsMigrationPlanItem>,
    blocked: &mut bool,
) {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            *blocked = true;
            inventory.push(scan_error(
                root.to_path_buf(),
                SkillsMigrationInventoryState::Unreadable,
            ));
            plan.push(repair_plan(SkillsMigrationReason::Unreadable));
            return;
        }
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        *blocked = true;
        inventory.push(scan_error(
            root.to_path_buf(),
            SkillsMigrationInventoryState::Invalid,
        ));
        plan.push(repair_plan(SkillsMigrationReason::InvalidLegacyState));
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => {
            *blocked = true;
            inventory.push(scan_error(
                root.to_path_buf(),
                SkillsMigrationInventoryState::Unreadable,
            ));
            plan.push(repair_plan(SkillsMigrationReason::Unreadable));
            return;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(_) => {
                *blocked = true;
                inventory.push(scan_error(
                    root.to_path_buf(),
                    SkillsMigrationInventoryState::Unreadable,
                ));
                plan.push(repair_plan(SkillsMigrationReason::Unreadable));
            }
        }
    }
    paths.sort();
    for path in paths {
        if is_ignored_root_metadata(&path) {
            continue;
        }
        if claimed.contains(&path) {
            continue;
        }
        claimed.insert(path.clone());
        let directory = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let state = classify_path(&path, managed_paths);
        let display = display_path(&path);
        let is_target_conflict = plan.iter().any(|item| {
            item.action == SkillsMigrationAction::CreateGlobalDeployment
                && item.to_location.as_deref() == Some(display.as_str())
        });
        let kind = if is_target_conflict {
            SkillsMigrationInventoryKind::TargetConflict
        } else if codex_legacy {
            SkillsMigrationInventoryKind::LegacyCodexEntry
        } else {
            SkillsMigrationInventoryKind::UnmanagedContent
        };
        inventory.push(SkillsMigrationInventoryItem {
            kind,
            directory: directory.clone(),
            consumer: codex_legacy.then_some(DeploymentConsumer::Codex),
            location: display_path(&path),
            managed_skill_id: None,
            enabled: None,
            state,
        });
        if let Some(directory) = directory {
            let (disposition, action, reason) = if codex_legacy
                && state == SkillsMigrationInventoryState::ManagedLink
            {
                (
                    SkillsMigrationDisposition::Perform,
                    SkillsMigrationAction::RemoveLegacyCodexLink,
                    SkillsMigrationReason::ProvenCcSwitchLink,
                )
            } else if matches!(
                state,
                SkillsMigrationInventoryState::Unreadable | SkillsMigrationInventoryState::Invalid
            ) {
                *blocked = true;
                (
                    SkillsMigrationDisposition::UserResolve,
                    SkillsMigrationAction::ResolveConflict,
                    SkillsMigrationReason::Unreadable,
                )
            } else if matches!(
                state,
                SkillsMigrationInventoryState::ForeignLink
                    | SkillsMigrationInventoryState::BrokenLink
                    | SkillsMigrationInventoryState::Occupied
            ) {
                (
                    SkillsMigrationDisposition::UserResolve,
                    SkillsMigrationAction::ResolveConflict,
                    SkillsMigrationReason::ForeignOrAmbiguous,
                )
            } else {
                (
                    SkillsMigrationDisposition::Preserve,
                    SkillsMigrationAction::PreserveContent,
                    SkillsMigrationReason::Unmanaged,
                )
            };
            plan.push(SkillsMigrationPlanItem {
                disposition,
                action,
                directory: Some(directory),
                consumer: codex_legacy.then_some(DeploymentConsumer::Codex),
                from_location: Some(display_path(&path)),
                to_location: None,
                reason,
                unsupported_consumers: Vec::new(),
            });
        }
    }
}

fn managed_paths<'a>(
    library: &[LibrarySkill],
    legacy: impl Iterator<Item = &'a InstalledSkill>,
    legacy_ssot: &Path,
) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    for skill in library {
        let path = get_app_config_dir().join("skills").join(&skill.directory);
        if classify_library_path(skill, &path) == SkillsMigrationInventoryState::Present {
            paths.insert(path);
        }
    }
    for skill in legacy.filter(|skill| valid_directory(&skill.directory)) {
        let path = legacy_ssot.join(&skill.directory);
        if is_real_directory(&path) {
            paths.insert(path);
        }
    }
    paths
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn backup_source_ready(
    action: SkillsMigrationAction,
    path: &Path,
    managed_paths: &BTreeSet<PathBuf>,
) -> bool {
    match action {
        SkillsMigrationAction::MoveToLibrary | SkillsMigrationAction::ReuseLibrary => {
            crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(path)
                .is_ok()
        }
        SkillsMigrationAction::RemoveLegacyCodexLink => {
            classify_path(path, managed_paths) == SkillsMigrationInventoryState::ManagedLink
        }
        _ => true,
    }
}

fn classify_library_path(skill: &LibrarySkill, path: &Path) -> SkillsMigrationInventoryState {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            match crate::services::skill::LibrarySkillAcquisitionService::inspect_source_directory(
                path,
            ) {
                Ok(observed) if observed.content_hash == skill.content_hash => {
                    SkillsMigrationInventoryState::Present
                }
                Ok(_) => SkillsMigrationInventoryState::Invalid,
                Err(_) => SkillsMigrationInventoryState::Invalid,
            }
        }
        Ok(_) => SkillsMigrationInventoryState::Invalid,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            SkillsMigrationInventoryState::Missing
        }
        Err(_) => SkillsMigrationInventoryState::Unreadable,
    }
}

fn classify_path(path: &Path, managed_paths: &BTreeSet<PathBuf>) -> SkillsMigrationInventoryState {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SkillsMigrationInventoryState::Missing
        }
        Err(_) => return SkillsMigrationInventoryState::Unreadable,
    };
    if metadata.file_type().is_symlink() {
        let target = match fs::read_link(path) {
            Ok(target) => target,
            Err(_) => return SkillsMigrationInventoryState::Unreadable,
        };
        let resolved = if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or_else(|| Path::new("")).join(target)
        };
        let canonical = match fs::canonicalize(&resolved) {
            Ok(canonical) => canonical,
            Err(_) => return SkillsMigrationInventoryState::BrokenLink,
        };
        if managed_paths.iter().any(|managed| {
            fs::canonicalize(managed)
                .map(|candidate| candidate == canonical)
                .unwrap_or(false)
        }) {
            SkillsMigrationInventoryState::ManagedLink
        } else {
            SkillsMigrationInventoryState::ForeignLink
        }
    } else if metadata.file_type().is_dir() {
        SkillsMigrationInventoryState::RealDirectory
    } else {
        SkillsMigrationInventoryState::Occupied
    }
}

fn recovery_exists() -> bool {
    let root = get_app_config_dir().join("skills-migration-backups");
    fs::read_dir(root)
        .map(|entries| {
            entries.flatten().any(|entry| {
                let path = entry.path();
                path.is_dir()
                    && path.join("database.db").is_file()
                    && path.join("backup-verified").is_file()
            })
        })
        .unwrap_or(false)
}

fn root_observations(roots: &FixedRoots) -> Vec<Observation> {
    let mut paths = roots
        .scan_roots
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| Observation {
            location: display_path(&path),
            fingerprint: root_fingerprint(&path),
        })
        .collect()
}

fn root_fingerprint(path: &Path) -> String {
    let mut hasher = Sha256::new();
    fingerprint_into(path, &mut hasher, true);
    format!("{:x}", hasher.finalize())
}

fn fingerprint_into(path: &Path, hasher: &mut Sha256, ignore_root_metadata: bool) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            hasher.update(format!("error:{:?}", error.kind()));
            return;
        }
    };
    hasher.update(metadata.dev().to_le_bytes());
    hasher.update(metadata.ino().to_le_bytes());
    if metadata.file_type().is_symlink() {
        hasher.update(b"link:");
        match fs::read_link(path) {
            Ok(target) => hasher.update(target.as_os_str().to_string_lossy().as_bytes()),
            Err(error) => hasher.update(format!("error:{:?}", error.kind())),
        }
    } else if metadata.file_type().is_file() {
        hasher.update(b"file:");
        match fs::read(path) {
            Ok(bytes) => hasher.update(bytes),
            Err(error) => hasher.update(format!("error:{:?}", error.kind())),
        }
    } else if metadata.file_type().is_dir() {
        hasher.update(b"dir:");
        let mut entries = match fs::read_dir(path) {
            Ok(entries) => {
                let mut observed = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => observed.push(entry),
                        Err(error) => {
                            hasher.update(b"entry-error:");
                            hasher.update(format!("{:?}", error.kind()));
                        }
                    }
                }
                observed
            }
            Err(error) => {
                hasher.update(format!("error:{:?}", error.kind()));
                return;
            }
        };
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if ignore_root_metadata && is_ignored_root_metadata(&entry.path()) {
                continue;
            }
            hasher.update(entry.file_name().to_string_lossy().as_bytes());
            fingerprint_into(&entry.path(), hasher, false);
        }
    } else {
        hasher.update(b"other");
    }
}

fn is_ignored_root_metadata(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    (matches!(name, ".DS_Store" | ".localized") || name.starts_with("._"))
        && fs::symlink_metadata(path)
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
}

fn plan_item_requires_manual_resolution(item: &SkillsMigrationPlanItem) -> bool {
    item.disposition == SkillsMigrationDisposition::UserResolve
}

#[allow(clippy::too_many_arguments)]
fn observation_token(
    pending: Option<&str>,
    snapshot: Option<&str>,
    legacy: &[InstalledSkill],
    library: &[LibrarySkill],
    inventory: &[SkillsMigrationInventoryItem],
    plan: &[SkillsMigrationPlanItem],
    backup: &SkillsMigrationBackupPlan,
    observations: &[Observation],
) -> Result<String> {
    let bytes = serde_json::to_vec(&(
        "skills-migration-preflight-v1",
        pending,
        snapshot,
        legacy,
        library,
        inventory,
        plan,
        backup,
        observations,
    ))?;
    Ok(format!(
        "skills-migration-preflight-v1:{:x}",
        Sha256::digest(bytes)
    ))
}

fn sort_inventory(items: &mut [SkillsMigrationInventoryItem]) {
    items.sort_by_cached_key(|item| serde_json::to_string(item).unwrap_or_default());
}

fn sort_plan(items: &mut [SkillsMigrationPlanItem]) {
    items.sort_by_cached_key(|item| serde_json::to_string(item).unwrap_or_default());
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}
