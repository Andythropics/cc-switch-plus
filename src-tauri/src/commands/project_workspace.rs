//! Project Workspace registration commands.

#[cfg(target_os = "macos")]
use crate::services::ProjectWorkspaceService;
#[cfg(target_os = "macos")]
use crate::store::AppState;
#[cfg(target_os = "macos")]
use std::path::Path;
#[cfg(target_os = "macos")]
use tauri::State;

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn inspectProjectWorkspace(
    path: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::WorkspaceRegistrationScan, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .inspect_path(Path::new(&path))
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn registerProjectWorkspace(
    path: String,
    display_name: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<crate::services::WorkspaceRegistration, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .register(Path::new(&path), display_name)
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn listProjectWorkspaces(
    include_archived: bool,
    app_state: State<'_, AppState>,
) -> Result<Vec<crate::services::ProjectWorkspace>, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .list(include_archived)
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn renameProjectWorkspace(
    workspace_id: String,
    display_name: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::ProjectWorkspace, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .rename(&workspace_id, display_name)
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn archiveProjectWorkspace(
    workspace_id: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::ProjectWorkspace, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .archive(&workspace_id)
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn restoreProjectWorkspace(
    workspace_id: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::ProjectWorkspace, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .restore(&workspace_id)
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn relocateProjectWorkspace(
    workspace_id: String,
    path: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::WorkspaceRelocation, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .relocate(&workspace_id, Path::new(&path))
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(non_snake_case)]
pub async fn forgetProjectWorkspace(
    workspace_id: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let db = app_state.db.clone();
    super::skill::run_skills_blocking(move || {
        ProjectWorkspaceService::new(db.clone())
            .forget(&workspace_id)
            .map_err(|error| error.to_string())
    })
    .await
}
