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
pub fn inspect_project_workspace(
    path: String,
    app_state: State<'_, AppState>,
) -> Result<crate::services::WorkspaceRegistrationScan, String> {
    ProjectWorkspaceService::new(app_state.db.clone())
        .inspect_path(Path::new(&path))
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn register_project_workspace(
    path: String,
    display_name: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<crate::services::WorkspaceRegistration, String> {
    ProjectWorkspaceService::new(app_state.db.clone())
        .register(Path::new(&path), display_name)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn list_project_workspaces(
    app_state: State<'_, AppState>,
) -> Result<Vec<crate::services::ProjectWorkspace>, String> {
    ProjectWorkspaceService::new(app_state.db.clone())
        .list()
        .map_err(|error| error.to_string())
}
