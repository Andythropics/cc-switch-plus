//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::error::format_skill_error;
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, LibrarySkill, LibrarySkillAcquisitionService,
    LibrarySourceKind, MigrationResult, Skill, SkillBackupEntry, SkillRepo, SkillService,
    SkillStorageLocation, SkillUninstallResult, SkillUpdateInfo, SkillsShSearchResult,
};
#[cfg(target_os = "macos")]
use crate::services::{
    ActivityPage, ActivityQuery, ActivityRecorder, LibrarySkillDeletionInspection,
    LibrarySkillDeletionIntent, LibrarySkillDeletionResult, LibrarySkillUpdateApplyIntent,
    LibrarySkillUpdateCheck, LibrarySkillUpdateResult, LibrarySkillUpdateService,
    ProjectSkillImportInspection, ProjectSkillImportIntent, ProjectSkillImportResult,
    ProjectSkillImportService,
};
use crate::services::{
    DeploymentBatch, DeploymentBatchResult, DeploymentInspectionResult, DeploymentQuery,
    SkillDeploymentService,
};
use crate::store::AppState;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tauri::State;

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
#[tauri::command]
pub fn get_installed_skills(app_state: State<'_, AppState>) -> Result<Vec<InstalledSkill>, String> {
    SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())
}

/// List private Library snapshots. This is intentionally separate from the
/// legacy installed/deployed Skill list.
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

#[tauri::command]
pub fn get_skill_backups() -> Result<Vec<SkillBackupEntry>, String> {
    SkillService::list_backups().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_skill_backup(backup_id: String) -> Result<bool, String> {
    SkillService::delete_backup(&backup_id).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 安装 Skill（新版统一安装）
///
/// 参数：
/// - skill: 从发现列表获取的技能信息
/// - current_app: 当前选中的应用，安装后默认启用该应用
#[tauri::command]
pub async fn install_skill_unified(
    skill: DiscoverableSkill,
    current_app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub fn uninstall_skill_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    SkillService::uninstall(&app_state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_skill_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;
    SkillService::restore_from_backup(&app_state.db, &backup_id, &app_type)
        .map_err(|e| e.to_string())
}

/// 切换 Skill 的应用启用状态
#[tauri::command]
pub fn toggle_skill_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    SkillService::toggle_app(&app_state.db, &id, &app_type, enabled).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 扫描未管理的 Skills
#[tauri::command]
pub fn scan_unmanaged_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<UnmanagedSkill>, String> {
    SkillService::scan_unmanaged(&app_state.db).map_err(|e| e.to_string())
}

/// 从应用目录导入 Skills
#[tauri::command]
pub fn import_skills_from_apps(
    imports: Vec<ImportSkillSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    SkillService::import_from_apps(&app_state.db, imports).map_err(|e| e.to_string())
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取）
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

/// 检查 Skills 更新
#[tauri::command]
pub async fn check_skill_updates(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillUpdateInfo>, String> {
    service
        .0
        .check_updates(&app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 更新单个 Skill
#[tauri::command]
pub async fn update_skill(
    id: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    service
        .0
        .update_skill(&app_state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

/// 迁移 Skill 存储位置
#[tauri::command]
pub async fn migrate_skill_storage(
    target: SkillStorageLocation,
    app_state: State<'_, AppState>,
) -> Result<MigrationResult, String> {
    SkillService::migrate_storage(&app_state.db, target).map_err(|e| e.to_string())
}

/// 搜索 skills.sh 公共目录
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

// ========== 兼容旧 API 的命令 ==========

/// 获取技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .list_skills(repos, &app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    // 新版本不再区分应用，统一返回所有技能
    let _ = parse_app_type(&app)?; // 验证 app 参数有效
    get_skills(service, app_state).await
}

/// 安装技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_skill_for_app("claude".to_string(), directory, service, app_state).await
}

/// 安装指定应用的技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;

    // 先获取技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let skills = service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| {
            let install_name = std::path::Path::new(&s.directory)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.directory.clone());
            install_name.eq_ignore_ascii_case(&directory)
                || s.directory.eq_ignore_ascii_case(&directory)
        })
        .ok_or_else(|| {
            format_skill_error(
                "SKILL_NOT_FOUND",
                &[("directory", &directory)],
                Some("checkRepoUrl"),
            )
        })?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// 卸载技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill(
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    uninstall_skill_for_app("claude".to_string(), directory, app_state)
}

/// 卸载指定应用的技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    let _ = parse_app_type(&app)?; // 验证参数

    // 通过 directory 找到对应的 skill id
    let skills = SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| s.directory.eq_ignore_ascii_case(&directory))
        .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;

    SkillService::uninstall(&app_state.db, &skill.id).map_err(|e| e.to_string())
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[tauri::command]
pub fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, String> {
    app_state.db.get_skill_repos().map_err(|e| e.to_string())
}

/// 添加技能仓库
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

/// 从 ZIP 文件安装 Skills
#[tauri::command]
pub fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    let app_type = parse_app_type(&current_app)?;
    let path = std::path::Path::new(&file_path);

    SkillService::install_from_zip(&app_state.db, path, &app_type).map_err(|e| e.to_string())
}
