//! Device-local Project Workspace persistence.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::project_workspace::{ProjectWorkspace, WorkspaceLifecycle, WorkspaceRootKind};
use rusqlite::{params, OptionalExtension, Row};
use std::path::PathBuf;

fn decode_workspace(row: &Row<'_>) -> rusqlite::Result<ProjectWorkspace> {
    let root_kind = match row.get::<_, String>(3)?.as_str() {
        "git_repository" => WorkspaceRootKind::GitRepository,
        "git_worktree" => WorkspaceRootKind::GitWorktree,
        "non_git" => WorkspaceRootKind::NonGit,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown workspace root kind: {other}").into(),
            ));
        }
    };
    let lifecycle = match row.get::<_, String>(4)?.as_str() {
        "active" => WorkspaceLifecycle::Active,
        "archived" => WorkspaceLifecycle::Archived,
        "unavailable" => WorkspaceLifecycle::Unavailable,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("unknown workspace lifecycle: {other}").into(),
            ));
        }
    };
    Ok(ProjectWorkspace {
        id: row.get(0)?,
        display_name: row.get(1)?,
        root_path: PathBuf::from(row.get::<_, String>(2)?),
        root_kind,
        lifecycle,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

const SELECT_WORKSPACE: &str = "SELECT id, display_name, root_path,
    root_kind, lifecycle, created_at, updated_at FROM project_workspaces";

fn root_kind_name(root_kind: WorkspaceRootKind) -> &'static str {
    match root_kind {
        WorkspaceRootKind::GitRepository => "git_repository",
        WorkspaceRootKind::GitWorktree => "git_worktree",
        WorkspaceRootKind::NonGit => "non_git",
    }
}

fn lifecycle_name(lifecycle: WorkspaceLifecycle) -> &'static str {
    match lifecycle {
        WorkspaceLifecycle::Active => "active",
        WorkspaceLifecycle::Archived => "archived",
        WorkspaceLifecycle::Unavailable => "unavailable",
    }
}

impl Database {
    pub fn save_project_workspace(&self, workspace: &ProjectWorkspace) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO project_workspaces
             (id, display_name, root_path, root_kind, lifecycle, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                workspace.id,
                workspace.display_name,
                workspace.root_path.display().to_string(),
                root_kind_name(workspace.root_kind),
                lifecycle_name(workspace.lifecycle),
                workspace.created_at,
                workspace.updated_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn list_project_workspaces(&self) -> Result<Vec<ProjectWorkspace>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&format!("{SELECT_WORKSPACE} ORDER BY display_name, id"))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], decode_workspace)
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn get_project_workspace(&self, id: &str) -> Result<Option<ProjectWorkspace>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("{SELECT_WORKSPACE} WHERE id = ?1"),
            [id],
            decode_workspace,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }
}
