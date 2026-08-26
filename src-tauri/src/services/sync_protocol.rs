//! Transport-agnostic sync protocol layer.
//!
//! Shared by WebDAV, S3, and future transports. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::process::Command;
use std::sync::OnceLock;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::error::AppError;
#[cfg(not(target_os = "macos"))]
use crate::services::skill::{skill_state_read_guard, skill_state_write_guard};

// Re-export archive functions for use by transport layers.
pub(crate) use super::webdav_sync::archive::{backup_current_skills, restore_skills_from_backup};
pub(crate) use super::webdav_sync::archive::{restore_skills_zip, zip_skills_ssot};

// ─── Protocol constants ──────────────────────────────────────

/// Wire-format identifier stored in remote manifests.
/// Retains historic "webdav" naming for backward compatibility with existing remotes.
pub(crate) const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
pub(crate) const PROTOCOL_VERSION: u32 = 2;
// macOS publishes the redesigned portable Library as a distinct database
// generation. Unsupported platforms retain the legacy db-v6 namespace, so a
// device can never download a snapshot with incompatible Skills semantics.
#[cfg(target_os = "macos")]
pub(crate) const DB_COMPAT_VERSION: u32 = 7;
#[cfg(not(target_os = "macos"))]
pub(crate) const DB_COMPAT_VERSION: u32 = 6;
pub(crate) const LEGACY_DB_COMPAT_VERSION: u32 = 5;
pub(crate) const REMOTE_DB_SQL: &str = "db.sql";
pub(crate) const REMOTE_SKILLS_ZIP: &str = "skills.zip";
pub(crate) const REMOTE_MANIFEST: &str = "manifest.json";
pub(crate) const MAX_DEVICE_NAME_LEN: usize = 64;
pub(crate) const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[cfg(target_os = "macos")]
const SYNC_RECOVERY_DIR: &str = ".sync-restore-recovery";
#[cfg(target_os = "macos")]
const SYNC_RECOVERY_MARKER: &str = "prepared.json";
#[cfg(target_os = "macos")]
const SYNC_RECOVERY_DATABASE: &str = "database.db";
#[cfg(target_os = "macos")]
const SYNC_RECOVERY_LIBRARY: &str = "library";

#[cfg(target_os = "macos")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncRecoveryMarker {
    library_existed: bool,
}

// ─── Sync operation lock ────────────────────────────────────

/// Serialize every snapshot upload/download across all transports.
///
/// WebDAV and S3 used to own separate mutexes, which allowed two transports to
/// restore the database and Skills SSOT concurrently. Keep the lock in this
/// transport-agnostic layer so future transports automatically share it too.
pub(crate) fn sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) async fn run_with_sync_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let _guard = sync_mutex().lock().await;
    operation.await
}

/// Tables whose changes make the remote configuration snapshot stale.
///
/// Keep this transport-agnostic so WebDAV and S3 cannot silently drift apart.
/// `model_pricing` is intentionally excluded while its local JSON sidecar is
/// the user-owned SSOT.
pub(crate) fn should_trigger_auto_sync_for_table(table: &str) -> bool {
    let normalized = table.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "providers"
            | "provider_endpoints"
            | "mcp_servers"
            | "prompts"
            | "skills"
            | "library_skills"
            | "skill_repos"
            | "profiles"
            | "settings"
            | "proxy_config"
    )
}

// ─── Error helpers ───────────────────────────────────────────

pub(crate) fn localized(
    key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
) -> AppError {
    AppError::localized(key, zh, en)
}

pub(crate) fn io_context_localized(
    _key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
    source: std::io::Error,
) -> AppError {
    let zh_msg = zh.into();
    let en_msg = en.into();
    AppError::IoContext {
        context: format!("{zh_msg} ({en_msg})"),
        source,
    }
}

// ─── Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncManifest {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_compat_version: Option<u32>,
    pub device_name: String,
    pub created_at: String,
    pub artifacts: BTreeMap<String, ArtifactMeta>,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArtifactMeta {
    pub sha256: String,
    pub size: u64,
}

pub(crate) struct LocalSnapshot {
    pub db_sql: Vec<u8>,
    pub skills_zip: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteLayout {
    Current,
    Legacy,
}

impl RemoteLayout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Legacy => "legacy",
        }
    }
}

// ─── Snapshot building ───────────────────────────────────────

pub(crate) fn build_local_snapshot(
    db: &crate::database::Database,
) -> Result<LocalSnapshot, AppError> {
    // Library metadata and files form one portable snapshot. Serialize them
    // with every other Library mutation so the SQL and ZIP cannot describe
    // different points in time.
    #[cfg(target_os = "macos")]
    let _library_guard =
        crate::services::skill::LibrarySkillAcquisitionService::lock_for_composite()
            .map_err(|error| AppError::Lock(error.to_string()))?;
    #[cfg(not(target_os = "macos"))]
    // Keep the DB's skill rows and the filesystem SSOT at one logical point in
    // time. Skill writers take the matching write guard around both mutations.
    let _skill_state_guard = skill_state_read_guard();

    // Export database to SQL string
    let sql_string = db.export_sql_string_for_sync()?;
    let db_sql = sql_string.into_bytes();

    // Pack skills into deterministic ZIP
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "sync.snapshot_tmpdir_failed",
            "创建快照临时目录失败",
            "Failed to create temporary directory for snapshot",
            e,
        )
    })?;
    let skills_zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    zip_skills_ssot(&skills_zip_path)?;
    let skills_zip = fs::read(&skills_zip_path).map_err(|e| AppError::io(&skills_zip_path, e))?;

    // Build artifact map and compute hashes
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&db_sql),
            size: db_sql.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&skills_zip),
            size: skills_zip.len() as u64,
        },
    );

    let snapshot_id = compute_snapshot_id(&artifacts);
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        snapshot_id,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        db_sql,
        skills_zip,
        manifest_bytes,
        manifest_hash,
    })
}

// ─── Manifest handling ───────────────────────────────────────

/// Compute a deterministic snapshot identity from artifact hashes.
///
/// BTreeMap iteration order is sorted by key, ensuring stability.
pub(crate) fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts: Vec<String> = artifacts
        .iter()
        .map(|(name, meta)| format!("{}:{}", name, meta.sha256))
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}

pub(crate) fn effective_db_compat_version(
    manifest: &SyncManifest,
    layout: RemoteLayout,
) -> Option<u32> {
    manifest
        .db_compat_version
        .or_else(|| (layout == RemoteLayout::Legacy).then_some(LEGACY_DB_COMPAT_VERSION))
}

pub(crate) fn validate_manifest_compat(
    manifest: &SyncManifest,
    layout: RemoteLayout,
) -> Result<(), AppError> {
    if manifest.format != PROTOCOL_FORMAT {
        return Err(localized(
            "sync.manifest_format_incompatible",
            format!("远端 manifest 格式不兼容: {}", manifest.format),
            format!(
                "Remote manifest format is incompatible: {}",
                manifest.format
            ),
        ));
    }
    if manifest.version != PROTOCOL_VERSION {
        return Err(localized(
            "sync.manifest_version_incompatible",
            format!(
                "远端 manifest 协议版本不兼容: v{} (本地 v{PROTOCOL_VERSION})",
                manifest.version
            ),
            format!(
                "Remote manifest protocol version is incompatible: v{} (local v{PROTOCOL_VERSION})",
                manifest.version
            ),
        ));
    }
    let Some(db_compat_version) = effective_db_compat_version(manifest, layout) else {
        return Err(localized(
            "sync.manifest_db_version_missing",
            "远端 manifest 缺少数据库兼容版本",
            "Remote manifest is missing the database compatibility version.",
        ));
    };
    match layout {
        RemoteLayout::Current if db_compat_version != DB_COMPAT_VERSION => {
            return Err(localized(
                "sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        RemoteLayout::Legacy if db_compat_version > DB_COMPAT_VERSION => {
            return Err(localized(
                "sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地最高支持 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local supports up to db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        _ => {}
    }
    Ok(())
}

// ─── Artifact verification ───────────────────────────────────

pub(crate) fn validate_artifact_size_limit(artifact_name: &str, size: u64) -> Result<(), AppError> {
    if size > MAX_SYNC_ARTIFACT_BYTES {
        let max_mb = MAX_SYNC_ARTIFACT_BYTES / 1024 / 1024;
        return Err(localized(
            "sync.artifact_too_large",
            format!("artifact {artifact_name} 超过下载上限（{} MB）", max_mb),
            format!(
                "Artifact {artifact_name} exceeds download limit ({} MB)",
                max_mb
            ),
        ));
    }
    Ok(())
}

/// Verify that downloaded artifact bytes match the expected size and SHA-256 hash.
pub(crate) fn verify_artifact(
    bytes: &[u8],
    artifact_name: &str,
    meta: &ArtifactMeta,
) -> Result<(), AppError> {
    // Quick size check before expensive hash
    if bytes.len() as u64 != meta.size {
        return Err(localized(
            "sync.artifact_size_mismatch",
            format!(
                "artifact {artifact_name} 大小不匹配 (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
            format!(
                "Artifact {artifact_name} size mismatch (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
        ));
    }

    let actual_hash = sha256_hex(bytes);
    if actual_hash != meta.sha256 {
        return Err(localized(
            "sync.artifact_hash_mismatch",
            format!(
                "artifact {artifact_name} SHA256 校验失败 (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
            format!(
                "Artifact {artifact_name} SHA256 verification failed (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
        ));
    }
    Ok(())
}

// ─── Snapshot application ────────────────────────────────────

pub(crate) fn apply_snapshot(
    db: &crate::database::Database,
    db_sql: &[u8],
    skills_zip: &[u8],
) -> Result<(), AppError> {
    // Keep the established Library -> Deployment lock order. Restoration
    // replaces Library files and metadata while deployment inspection must
    // not observe the intermediate state.
    #[cfg(target_os = "macos")]
    let _library_guard =
        crate::services::skill::LibrarySkillAcquisitionService::lock_for_composite()
            .map_err(|error| AppError::Lock(error.to_string()))?;
    #[cfg(target_os = "macos")]
    let _deployment_guard =
        crate::services::skill_deployment::SkillDeploymentService::lock_for_composite()
            .map_err(|error| AppError::Lock(error.to_string()))?;

    let sql_str = std::str::from_utf8(db_sql).map_err(|e| {
        localized(
            "sync.sql_not_utf8",
            format!("SQL 非 UTF-8: {e}"),
            format!("SQL is not valid UTF-8: {e}"),
        )
    })?;
    #[cfg(not(target_os = "macos"))]
    // Exclude installs, uninstalls, updates, and local projection while Skills
    // are backed up/replaced and the corresponding database snapshot is applied.
    let _skill_state_guard = skill_state_write_guard();

    #[cfg(target_os = "macos")]
    {
        recover_interrupted_snapshot_locked(db)?;
        prepare_snapshot_recovery(db)?;

        if let Err(restore_error) = restore_skills_zip(skills_zip) {
            return rollback_snapshot_error(db, restore_error);
        }
        if let Err(import_error) = db.import_sql_string_for_sync(sql_str) {
            return rollback_snapshot_error(db, import_error);
        }

        commit_snapshot_recovery()?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let backup_dir = tempdir().map_err(|error| {
            io_context_localized(
                "sync.snapshot_tmpdir_failed",
                "创建 Skills 回滚临时目录失败",
                "Failed to create temporary directory for Skills rollback",
                error,
            )
        })?;
        let skills_existed = backup_current_skills(backup_dir.path())?;
        restore_skills_zip(skills_zip)?;
        if let Err(db_error) = db.import_sql_string_for_sync(sql_str) {
            if let Err(rollback_error) =
                restore_skills_from_backup(backup_dir.path(), skills_existed)
            {
                return Err(localized(
                    "sync.db_import_and_rollback_failed",
                    format!("导入数据库失败: {db_error}; 同时回滚 Skills 失败: {rollback_error}"),
                    format!("Database import failed: {db_error}; skills rollback also failed: {rollback_error}"),
                ));
            }
            return Err(db_error);
        }
        Ok(())
    }
}

/// Complete a previously interrupted macOS Library + database replacement.
///
/// The recovery marker is the commit boundary. While it exists, the old
/// database and Library remain authoritative and are restored as one pair.
#[cfg(target_os = "macos")]
pub(crate) fn recover_interrupted_snapshot(db: &crate::database::Database) -> Result<(), AppError> {
    let _library_guard =
        crate::services::skill::LibrarySkillAcquisitionService::lock_for_composite()
            .map_err(|error| AppError::Lock(error.to_string()))?;
    let _deployment_guard =
        crate::services::skill_deployment::SkillDeploymentService::lock_for_composite()
            .map_err(|error| AppError::Lock(error.to_string()))?;
    recover_interrupted_snapshot_locked(db)
}

#[cfg(target_os = "macos")]
fn sync_recovery_root() -> std::path::PathBuf {
    crate::config::get_app_config_dir().join(SYNC_RECOVERY_DIR)
}

#[cfg(target_os = "macos")]
fn prepare_snapshot_recovery(db: &crate::database::Database) -> Result<(), AppError> {
    let root = sync_recovery_root();
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|error| AppError::io(&root, error))?;
    }
    fs::create_dir_all(&root).map_err(|error| AppError::io(&root, error))?;
    if let Some(parent) = root.parent() {
        sync_directory(parent)?;
    }

    let database_backup = root.join(SYNC_RECOVERY_DATABASE);
    let library_backup = root.join(SYNC_RECOVERY_LIBRARY);
    let prepared = (|| {
        db.backup_database_snapshot_file(&database_backup)?;
        fs::File::open(&database_backup)
            .and_then(|file| file.sync_all())
            .map_err(|error| AppError::io(&database_backup, error))?;
        let library_existed = backup_current_skills(&library_backup)?;
        let marker = SyncRecoveryMarker { library_existed };
        let marker_bytes =
            serde_json::to_vec(&marker).map_err(|source| AppError::JsonSerialize { source })?;
        let staged_marker = root.join(format!("{SYNC_RECOVERY_MARKER}.tmp"));
        fs::write(&staged_marker, marker_bytes)
            .and_then(|()| fs::File::open(&staged_marker)?.sync_all())
            .map_err(|error| AppError::io(&staged_marker, error))?;
        fs::rename(&staged_marker, root.join(SYNC_RECOVERY_MARKER))
            .map_err(|error| AppError::io(&root, error))?;
        sync_directory(&root)?;
        Ok(())
    })();

    if prepared.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    prepared
}

#[cfg(target_os = "macos")]
fn recover_interrupted_snapshot_locked(db: &crate::database::Database) -> Result<(), AppError> {
    let root = sync_recovery_root();
    let marker_path = root.join(SYNC_RECOVERY_MARKER);
    if !marker_path.exists() {
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|error| AppError::io(&root, error))?;
        }
        return Ok(());
    }

    let marker: SyncRecoveryMarker = serde_json::from_slice(
        &fs::read(&marker_path).map_err(|error| AppError::io(&marker_path, error))?,
    )
    .map_err(|source| AppError::json(&marker_path, source))?;
    let database_backup = root.join(SYNC_RECOVERY_DATABASE);
    let library_backup = root.join(SYNC_RECOVERY_LIBRARY);

    db.restore_database_snapshot_file(&database_backup)?;
    restore_skills_from_backup(&library_backup, marker.library_existed)?;
    commit_snapshot_recovery()
}

#[cfg(target_os = "macos")]
fn rollback_snapshot_error(
    db: &crate::database::Database,
    error: AppError,
) -> Result<(), AppError> {
    match recover_interrupted_snapshot_locked(db) {
        Ok(()) => Err(error),
        Err(rollback_error) => Err(localized(
            "sync.db_import_and_rollback_failed",
            format!("应用同步快照失败: {error}; 同时恢复旧快照失败: {rollback_error}"),
            format!(
                "Applying the sync snapshot failed: {error}; restoring the previous snapshot also failed: {rollback_error}"
            ),
        )),
    }
}

#[cfg(target_os = "macos")]
fn commit_snapshot_recovery() -> Result<(), AppError> {
    let root = sync_recovery_root();
    let marker = root.join(SYNC_RECOVERY_MARKER);
    if marker.exists() {
        fs::remove_file(&marker).map_err(|error| AppError::io(&marker, error))?;
        sync_directory(&root)?;
    }
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|error| AppError::io(&root, error))?;
        if let Some(parent) = root.parent() {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn sync_directory(path: &std::path::Path) -> Result<(), AppError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| AppError::io(path, error))
}

// ─── Utilities ───────────────────────────────────────────────

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn detect_system_device_name() -> Option<String> {
    let env_name = ["CC_SWITCH_DEVICE_NAME", "COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| normalize_device_name(&value));

    if env_name.is_some() {
        return env_name;
    }

    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let hostname = String::from_utf8(output.stdout).ok()?;
    normalize_device_name(&hostname)
}

pub(crate) fn normalize_device_name(raw: &str) -> Option<String> {
    let compact = raw
        .chars()
        .fold(String::with_capacity(raw.len()), |mut acc, ch| {
            if ch.is_whitespace() {
                acc.push(' ');
            } else if !ch.is_control() {
                acc.push(ch);
            }
            acc
        });
    let normalized = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }

    let limited = trimmed
        .chars()
        .take(MAX_DEVICE_NAME_LEN)
        .collect::<String>();
    if limited.is_empty() {
        None
    } else {
        Some(limited)
    }
}

// ─── Sync status persistence ─────────────────────────────────

pub(crate) fn persist_sync_success_best_effort<S, F>(
    settings: &mut S,
    manifest_hash: String,
    etag: Option<String>,
    persist_fn: F,
) -> bool
where
    F: FnOnce(&mut S, String, Option<String>) -> Result<(), AppError>,
{
    match persist_fn(settings, manifest_hash, etag) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("[Sync] Persist sync status failed, keep operation success: {err}");
            false
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    #[serial_test::serial]
    fn remote_library_deletion_with_local_deployment_rolls_back_the_pair() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let remote = crate::database::Database::memory().expect("remote database");
        {
            let conn = crate::database::lock_conn!(remote.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote', '{}', '{}')",
                [],
            )?;
        }
        let remote_sql = remote
            .export_sql_string_for_sync()
            .expect("export remote snapshot");
        let remote_zip_path = temp.path().join("remote-skills.zip");
        zip_skills_ssot(&remote_zip_path).expect("archive empty remote Library");
        let remote_zip = fs::read(&remote_zip_path).expect("read remote Library archive");

        let local = crate::database::Database::memory().expect("local database");
        {
            let conn = crate::database::lock_conn!(local.conn);
            conn.execute(
                "INSERT INTO library_skills (
                     id, directory, display_name, description, source_json,
                     compatibility_json, content_hash, acquired_at, updated_at
                 ) VALUES (
                     'library-1', 'review', 'Review', NULL, '{\"kind\":\"local_import\"}',
                     '{\"claude\":true,\"codex\":true}', 'hash-1', 1, 1
                 )",
                [],
            )
            .expect("seed local Library metadata");
            conn.execute(
                "INSERT INTO skill_deployments (
                     id, library_skill_id, consumer, workspace_kind,
                     library_directory, workspace_id, created_at, updated_at
                 ) VALUES ('deployment-1', 'library-1', 'claude', 'global', 'review', '', 1, 1)",
                [],
            )
            .expect("seed local Deployment");
        }
        let library = temp.path().join(".cc-switch/skills/review");
        fs::create_dir_all(&library).expect("create local Library");
        fs::write(library.join("SKILL.md"), "local Library").expect("write local Library");

        let error = apply_snapshot(&local, remote_sql.as_bytes(), &remote_zip)
            .expect_err("remote deletion must be rejected");
        assert!(
            error.to_string().contains("undeploy or Forget"),
            "unexpected sync blocker: {error}"
        );

        let conn = crate::database::lock_conn!(local.conn);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM library_skills", [], |row| row
                .get::<_, i64>(0))
                .expect("count Library rows"),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM skill_deployments", [], |row| row
                .get::<_, i64>(0))
                .expect("count Deployments"),
            1
        );
        drop(conn);
        assert_eq!(
            fs::read_to_string(library.join("SKILL.md")).expect("read restored Library"),
            "local Library"
        );
        assert!(!sync_recovery_root().exists());

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[serial_test::serial]
    fn interrupted_snapshot_recovery_restores_database_and_library_pair() {
        let temp = tempdir().expect("tempdir");
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let db = crate::database::Database::memory().expect("database");
        db.set_setting("sync_recovery_probe", "old")
            .expect("seed old database state");
        let library = temp.path().join(".cc-switch/skills/example");
        fs::create_dir_all(&library).expect("create old Library");
        fs::write(library.join("SKILL.md"), "old Library").expect("write old Library content");

        prepare_snapshot_recovery(&db).expect("prepare durable recovery");
        assert!(sync_recovery_root().join(SYNC_RECOVERY_MARKER).is_file());

        db.set_setting("sync_recovery_probe", "new")
            .expect("simulate imported database");
        fs::remove_dir_all(temp.path().join(".cc-switch/skills")).expect("remove old Library");
        fs::create_dir_all(&library).expect("create new Library");
        fs::write(library.join("SKILL.md"), "new Library").expect("write new Library content");

        recover_interrupted_snapshot(&db).expect("recover interrupted snapshot");

        assert_eq!(
            db.get_setting("sync_recovery_probe")
                .expect("read restored database"),
            Some("old".to_string())
        );
        assert_eq!(
            fs::read_to_string(library.join("SKILL.md")).expect("read restored Library"),
            "old Library"
        );
        assert!(!sync_recovery_root().exists());

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[tokio::test]
    async fn webdav_and_s3_operations_share_one_sync_mutex() {
        let webdav_lock = crate::services::webdav_sync::sync_mutex();
        let s3_lock = crate::services::s3_sync::sync_mutex();
        assert!(
            std::ptr::eq(webdav_lock, s3_lock),
            "every transport must expose the same global sync lock"
        );

        let guard = webdav_lock.lock().await;
        assert!(s3_lock.try_lock().is_err());
        drop(guard);
        assert!(s3_lock.try_lock().is_ok());
    }

    fn artifact(sha256: &str, size: u64) -> ArtifactMeta {
        ArtifactMeta {
            sha256: sha256.to_string(),
            size,
        }
    }

    #[test]
    fn auto_sync_table_filter_covers_shared_configuration() {
        for table in [
            "providers",
            "provider_endpoints",
            "mcp_servers",
            "prompts",
            "skills",
            "library_skills",
            "skill_repos",
            "profiles",
            "settings",
            "proxy_config",
        ] {
            assert!(
                should_trigger_auto_sync_for_table(table),
                "{table} should trigger an automatic snapshot upload"
            );
        }

        assert!(should_trigger_auto_sync_for_table("  PROFILES  "));
        for table in [
            "proxy_request_logs",
            "provider_health",
            "session_log_sync",
            "model_pricing",
        ] {
            assert!(
                !should_trigger_auto_sync_for_table(table),
                "{table} should not trigger automatic snapshot upload"
            );
        }
    }

    #[test]
    fn snapshot_id_is_stable() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc123", 100));
        artifacts.insert("skills.zip".to_string(), artifact("def456", 200));

        let id1 = compute_snapshot_id(&artifacts);
        let id2 = compute_snapshot_id(&artifacts);
        assert_eq!(id1, id2);
    }

    #[test]
    fn snapshot_id_changes_with_artifacts() {
        let mut a1 = BTreeMap::new();
        a1.insert("db.sql".to_string(), artifact("hash-a", 1));

        let mut a2 = BTreeMap::new();
        a2.insert("db.sql".to_string(), artifact("hash-b", 1));

        assert_ne!(compute_snapshot_id(&a1), compute_snapshot_id(&a2));
    }

    #[test]
    fn sha256_hex_is_correct() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn persist_best_effort_returns_true_on_success() {
        let mut dummy = ();
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            Some("etag".to_string()),
            |_settings, _hash, _etag| Ok(()),
        );
        assert!(ok);
    }

    #[test]
    fn persist_best_effort_returns_false_on_error() {
        let mut dummy = ();
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            None,
            |_settings, _hash, _etag| Err(AppError::Config("boom".to_string())),
        );
        assert!(!ok);
    }

    fn manifest_with(format: &str, version: u32, db_compat_version: Option<u32>) -> SyncManifest {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc", 1));
        artifacts.insert("skills.zip".to_string(), artifact("def", 2));
        SyncManifest {
            format: format.to_string(),
            version,
            db_compat_version,
            device_name: "My MacBook".to_string(),
            created_at: "2026-02-12T00:00:00Z".to_string(),
            artifacts,
            snapshot_id: "snap-1".to_string(),
        }
    }

    #[test]
    fn validate_manifest_compat_accepts_supported_manifest() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_format() {
        let manifest = manifest_with("other-format", PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_version() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION + 1,
            Some(DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_accepts_legacy_manifest_without_db_compat() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, None);
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Legacy).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_current_manifest_with_wrong_db_compat() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(LEGACY_DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_legacy_manifest_from_newer_db_generation() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION + 1),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Legacy).is_err());
    }

    #[test]
    fn effective_db_compat_version_defaults_legacy_layout_to_v5() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, None);
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLayout::Legacy),
            Some(LEGACY_DB_COMPAT_VERSION)
        );
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLayout::Current),
            None
        );
    }

    #[test]
    fn normalize_device_name_returns_none_for_blank_input() {
        assert_eq!(normalize_device_name("   \n\t  "), None);
    }

    #[test]
    fn normalize_device_name_collapses_whitespace_and_drops_control_chars() {
        assert_eq!(
            normalize_device_name("  Mac\tBook \n Pro\u{0007} "),
            Some("Mac Book Pro".to_string())
        );
    }

    #[test]
    fn normalize_device_name_truncates_to_max_len() {
        let long = "a".repeat(80);
        assert_eq!(normalize_device_name(&long).map(|s| s.len()), Some(64));
    }

    #[test]
    fn manifest_serialization_uses_device_name_only() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        let value = serde_json::to_value(&manifest).expect("serialize manifest");
        assert!(
            value.get("deviceName").is_some(),
            "manifest should contain deviceName"
        );
        assert_eq!(
            value.get("dbCompatVersion").and_then(|v| v.as_u64()),
            Some(DB_COMPAT_VERSION as u64)
        );
        assert!(
            value.get("deviceId").is_none(),
            "manifest should not contain deviceId"
        );
    }

    #[test]
    fn validate_artifact_size_limit_rejects_oversized_artifacts() {
        let err = validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES + 1)
            .expect_err("artifact larger than limit should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_artifact_size_limit_accepts_limit_boundary() {
        assert!(validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES).is_ok());
    }

    #[test]
    fn verify_artifact_rejects_size_mismatch() {
        let meta = artifact("abc123", 100);
        let bytes = vec![0u8; 50];
        let err = verify_artifact(&bytes, "test.bin", &meta)
            .expect_err("size mismatch should be rejected");
        assert!(
            err.to_string().contains("mismatch") || err.to_string().contains("不匹配"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn verify_artifact_rejects_hash_mismatch() {
        let meta = ArtifactMeta {
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            size: 5,
        };
        let bytes = b"hello";
        let err = verify_artifact(bytes, "test.bin", &meta)
            .expect_err("hash mismatch should be rejected");
        assert!(
            err.to_string().contains("verification failed") || err.to_string().contains("校验失败"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn verify_artifact_accepts_matching_data() {
        let data = b"hello";
        let meta = ArtifactMeta {
            sha256: sha256_hex(data),
            size: data.len() as u64,
        };
        assert!(verify_artifact(data, "test.bin", &meta).is_ok());
    }

    #[test]
    fn skills_sync_namespace_matches_platform_contract() {
        #[cfg(target_os = "macos")]
        assert_eq!(DB_COMPAT_VERSION, 7);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(DB_COMPAT_VERSION, 6);
    }
}
