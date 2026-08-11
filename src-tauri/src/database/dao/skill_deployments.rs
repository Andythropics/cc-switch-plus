//! Durable desired state for Library Skill deployments.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill_deployment::{
    DeploymentConsumer, DeploymentTarget, DesiredDeployment, WorkspaceKind,
};
use rusqlite::{params, OptionalExtension, Row};

fn decode_deployment(row: &Row<'_>) -> rusqlite::Result<DesiredDeployment> {
    let workspace_kind: String = row.get(3)?;
    let consumer: String = row.get(2)?;
    let workspace = match workspace_kind.as_str() {
        "global" => WorkspaceKind::Global,
        "project" => WorkspaceKind::Project,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown workspace kind: {other}").into(),
            ));
        }
    };
    let consumer = match consumer.as_str() {
        "claude" => DeploymentConsumer::Claude,
        "codex" => DeploymentConsumer::Codex,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                format!("unknown deployment consumer: {other}").into(),
            ));
        }
    };

    Ok(DesiredDeployment {
        id: row.get(0)?,
        library_skill_id: row.get(1)?,
        library_directory: row.get(4)?,
        target: DeploymentTarget {
            consumer,
            workspace,
            workspace_id: row.get(5)?,
        },
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

const SELECT_DEPLOYMENT: &str = "SELECT id, library_skill_id, consumer,
        workspace_kind, library_directory, workspace_id, created_at, updated_at
     FROM skill_deployments";

fn consumer_name(consumer: DeploymentConsumer) -> &'static str {
    match consumer {
        DeploymentConsumer::Claude => "claude",
        DeploymentConsumer::Codex => "codex",
    }
}

fn workspace_name(workspace: WorkspaceKind) -> &'static str {
    match workspace {
        WorkspaceKind::Global => "global",
        WorkspaceKind::Project => "project",
    }
}

impl Database {
    pub fn save_skill_deployment(&self, deployment: &DesiredDeployment) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO skill_deployments
             (id, library_skill_id, consumer, workspace_kind, library_directory,
              workspace_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                deployment.id,
                deployment.library_skill_id,
                consumer_name(deployment.target.consumer),
                workspace_name(deployment.target.workspace),
                deployment.library_directory,
                deployment.target.workspace_id,
                deployment.created_at,
                deployment.updated_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn list_skill_deployments(&self) -> Result<Vec<DesiredDeployment>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&format!("{SELECT_DEPLOYMENT} ORDER BY created_at, id"))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], decode_deployment)
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn get_skill_deployment(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<Option<DesiredDeployment>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!(
                "{SELECT_DEPLOYMENT}
                 WHERE library_skill_id = ?1 AND consumer = ?2
                   AND workspace_kind = ?3 AND workspace_id = ?4"
            ),
            params![
                library_skill_id,
                consumer_name(target.consumer),
                workspace_name(target.workspace),
                target.workspace_id,
            ],
            decode_deployment,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn delete_skill_deployment(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute("DELETE FROM skill_deployments WHERE id = ?1", [id])
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(changed > 0)
    }

    /// Failure injection used by the real-filesystem deployment integration
    /// tests. It is compiled only for debug/test builds and is not part of the
    /// runtime command surface.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn fail_skill_deployment_inserts_for_test(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS test_fail_skill_deployment_insert;
             CREATE TRIGGER test_fail_skill_deployment_insert
             BEFORE INSERT ON skill_deployments
             BEGIN SELECT RAISE(ABORT, 'injected deployment insert failure'); END;",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn fail_skill_deployment_deletes_for_test(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS test_fail_skill_deployment_delete;
             CREATE TRIGGER test_fail_skill_deployment_delete
             BEFORE DELETE ON skill_deployments
             BEGIN SELECT RAISE(ABORT, 'injected deployment delete failure'); END;",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }
}
