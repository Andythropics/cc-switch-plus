//! Skills 命令层
//!
//! The public command surface is the macOS Library, Deployment, migration,
//! recovery, and read-only Catalog boundary. Legacy activation remains an
//! internal migration input only.

#[cfg(target_os = "macos")]
use crate::services::skill::{
    DiscoverableSkill, LibrarySkill, LibrarySkillAcquisitionService, LibrarySourceKind, SkillRepo,
    SkillService, SkillsShSearchResult,
};
#[cfg(target_os = "macos")]
use crate::services::{
    ActivityPage, ActivityQuery, ActivityRecorder, DeploymentBatch, DeploymentBatchResult,
    DeploymentInspectionResult, DeploymentQuery, DeploymentRecoveryInspectionResult,
    DeploymentRecoveryQuery, DeploymentRecoveryService, LibrarySkillDeletionInspection,
    LibrarySkillDeletionIntent, LibrarySkillDeletionResult, LibrarySkillUpdateApplyIntent,
    LibrarySkillUpdateCheck, LibrarySkillUpdateResult, LibrarySkillUpdateService,
    ProjectSkillImportInspection, ProjectSkillImportIntent, ProjectSkillImportResult,
    ProjectSkillImportService, SkillDeploymentService, SkillsMigrationExecutionResult,
    SkillsMigrationExecutionService, SkillsMigrationIntent, SkillsMigrationPreflight,
    SkillsMigrationPreviewService, SkillsMigrationRestoreIntent, SkillsMigrationRevealIntent,
};
#[cfg(target_os = "macos")]
use crate::store::AppState;
#[cfg(target_os = "macos")]
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use tauri::{AppHandle, State};
#[cfg(target_os = "macos")]
use tauri_plugin_opener::OpenerExt;

/// SkillService 状态包装
#[cfg(target_os = "macos")]
pub struct SkillServiceState(pub Arc<SkillService>);

/// List private Library snapshots. This is intentionally separate from the
/// legacy installed/deployed Skill list.
#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn getLibrarySkills(app_state: State<'_, AppState>) -> Result<Vec<LibrarySkill>, String> {
    LibrarySkillAcquisitionService::ensure_supported_platform()
        .map_err(|error| error.to_string())?;
    app_state
        .db
        .list_library_skills()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn listSkillActivity(
    query: Option<ActivityQuery>,
    app_state: State<'_, AppState>,
) -> Result<ActivityPage, String> {
    ActivityRecorder::new(app_state.db.clone())
        .list(query.unwrap_or_default())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn inspectDeploymentRecovery(
    query: Option<DeploymentRecoveryQuery>,
    app_state: State<'_, AppState>,
) -> Result<DeploymentRecoveryInspectionResult, String> {
    DeploymentRecoveryService::new(app_state.db.clone())
        .inspect(query.unwrap_or_default())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn inspectSkillsMigrationPreflight(
    app_state: State<'_, AppState>,
) -> Result<SkillsMigrationPreflight, String> {
    SkillsMigrationPreviewService::new(app_state.db.clone())
        .inspect()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn revealSkillsMigrationPlanItem(
    intent: SkillsMigrationRevealIntent,
    app_state: State<'_, AppState>,
    app: AppHandle,
) -> Result<bool, String> {
    let directory = SkillsMigrationPreviewService::new(app_state.db.clone())
        .resolve_reveal_directory(intent)
        .map_err(|error| error.to_string())?;
    app.opener()
        .open_path(directory.to_string_lossy().to_string(), None::<String>)
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn applySkillsMigration(
    intent: SkillsMigrationIntent,
    app_state: State<'_, AppState>,
) -> Result<SkillsMigrationExecutionResult, String> {
    SkillsMigrationExecutionService::new(app_state.db.clone())
        .start(intent)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn resumeSkillsMigration(
    app_state: State<'_, AppState>,
) -> Result<SkillsMigrationExecutionResult, String> {
    SkillsMigrationExecutionService::new(app_state.db.clone())
        .resume()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn restoreSkillsMigrationBackup(
    backupId: String,
    app_state: State<'_, AppState>,
) -> Result<SkillsMigrationExecutionResult, String> {
    SkillsMigrationExecutionService::new(app_state.db.clone())
        .restore(SkillsMigrationRestoreIntent {
            backup_id: backupId,
        })
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn inspectSkillDeployments(
    query: Option<DeploymentQuery>,
    app_state: State<'_, AppState>,
) -> Result<DeploymentInspectionResult, String> {
    SkillDeploymentService::new(app_state.db.clone())
        .inspect(query.unwrap_or_default())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn applySkillDeployments(
    batch: DeploymentBatch,
    app_state: State<'_, AppState>,
) -> Result<DeploymentBatchResult, String> {
    SkillDeploymentService::new(app_state.db.clone())
        .apply(batch)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn inspectProjectSkillImports(
    workspaceId: String,
    app_state: State<'_, AppState>,
) -> Result<ProjectSkillImportInspection, String> {
    ProjectSkillImportService::new(app_state.db.clone())
        .inspect(&workspaceId)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn applyProjectSkillImport(
    intent: ProjectSkillImportIntent,
    app_state: State<'_, AppState>,
) -> Result<ProjectSkillImportResult, String> {
    ProjectSkillImportService::new(app_state.db.clone())
        .apply(intent)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn acquireLibrarySkill(
    skill: DiscoverableSkill,
    source_kind: LibrarySourceKind,
    directory_name: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkill, String> {
    LibrarySkillAcquisitionService::acquire_discoverable(
        &app_state.db,
        &skill,
        source_kind,
        directory_name.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn acquireLibrarySkillsFromZip(
    file_path: String,
    directory_names: HashMap<String, String>,
    app_state: State<'_, AppState>,
) -> Result<Vec<LibrarySkill>, String> {
    LibrarySkillAcquisitionService::acquire_from_zip(
        &app_state.db,
        std::path::Path::new(&file_path),
        &directory_names,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn updateLibrarySkillMetadata(
    id: String,
    display_name: String,
    description: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkill, String> {
    LibrarySkillAcquisitionService::update_display_metadata(
        &app_state.db,
        &id,
        &display_name,
        description.as_deref(),
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn checkLibrarySkillUpdate(
    librarySkillId: String,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkillUpdateCheck, String> {
    LibrarySkillAcquisitionService::ensure_supported_platform()
        .map_err(|error| error.to_string())?;
    let skill = app_state
        .db
        .get_library_skill_by_id(&librarySkillId)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Library Skill not found: {librarySkillId}"))?;
    if !matches!(
        skill.source.kind,
        LibrarySourceKind::Git | LibrarySourceKind::Marketplace
    ) {
        return LibrarySkillUpdateService::check_not_updatable(&app_state.db, &librarySkillId)
            .map_err(|error| error.to_string());
    }
    let (_snapshot, repository_root) =
        LibrarySkillAcquisitionService::download_repository_snapshot_exact(&skill.source)
            .await
            .map_err(|error| error.to_string())?;
    LibrarySkillUpdateService::stage_from_repository_snapshot(
        &app_state.db,
        &librarySkillId,
        &repository_root,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn applyLibrarySkillUpdate(
    intent: LibrarySkillUpdateApplyIntent,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkillUpdateResult, String> {
    LibrarySkillUpdateService::apply(&app_state.db, intent).map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn inspectLibrarySkillDeletion(
    librarySkillId: String,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkillDeletionInspection, String> {
    LibrarySkillUpdateService::inspect_deletion(&app_state.db, &librarySkillId)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub fn deleteLibrarySkill(
    intent: LibrarySkillDeletionIntent,
    app_state: State<'_, AppState>,
) -> Result<LibrarySkillDeletionResult, String> {
    LibrarySkillUpdateService::delete(&app_state.db, intent).map_err(|error| error.to_string())
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取）
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn discover_available_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableSkill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())
}

/// 搜索 skills.sh 公共目录
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn search_skills_sh(
    query: String,
    limit: usize,
    offset: usize,
) -> Result<SkillsShSearchResult, String> {
    SkillService::search_skills_sh(&query, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, String> {
    app_state.db.get_skill_repos().map_err(|e| e.to_string())
}

/// 添加技能仓库
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn add_skill_repo(repo: SkillRepo, app_state: State<'_, AppState>) -> Result<bool, String> {
    // 整个结构体由前端反序列化而来，owner/name/branch 会被拼进归档下载 URL。
    // 主防线在 download_repo，这里让非法值当场报错而不是沉淀进表。
    SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)
        .map_err(|e| e.to_string())?;
    app_state
        .db
        .save_skill_repo(&repo)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 删除技能仓库
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn remove_skill_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .delete_skill_repo(&owner, &name)
        .map_err(|e| e.to_string())?;
    Ok(true)
}
