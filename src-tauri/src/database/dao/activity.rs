//! Device-local structured Skills activity DAO.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::activity::{
    ActivityBatchContext, ActivityDetailCode, ActivityEventInput, ActivityPage, ActivityQuery,
    ActivityRecord, ActivityTarget,
};
use rusqlite::{params_from_iter, types::Value, Row};

fn decode_activity(row: &Row<'_>) -> rusqlite::Result<ActivityRecord> {
    let operation =
        crate::services::activity::ActivityOperation::from_str(&row.get::<_, String>(1)?)
            .ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    1,
                    "operation".into(),
                    rusqlite::types::Type::Text,
                )
            })?;
    let reason = crate::services::activity::ActivityReason::from_str(&row.get::<_, String>(2)?)
        .ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(2, "reason".into(), rusqlite::types::Type::Text)
        })?;
    let outcome = crate::services::activity::ActivityOutcome::from_str(&row.get::<_, String>(3)?)
        .ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(3, "outcome".into(), rusqlite::types::Type::Text)
    })?;
    let actor = crate::services::activity::ActivityActor::from_str(&row.get::<_, String>(4)?)
        .ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(4, "actor".into(), rusqlite::types::Type::Text)
        })?;
    let trigger = crate::services::activity::ActivityTrigger::from_str(&row.get::<_, String>(5)?)
        .ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(5, "trigger".into(), rusqlite::types::Type::Text)
    })?;
    let detail_code =
        ActivityDetailCode::from_str(&row.get::<_, String>(14)?).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(
                14,
                "detail_code".into(),
                rusqlite::types::Type::Text,
            )
        })?;
    let batch_id = row.get::<_, Option<String>>(12)?;
    let batch_index = row.get::<_, Option<i64>>(13)?;
    let batch_count = row.get::<_, Option<i64>>(15)?;
    let batch = match (batch_id, batch_index, batch_count) {
        (Some(batch_id), Some(item_index), Some(item_count)) => Some(ActivityBatchContext {
            batch_id,
            item_index: item_index as u32,
            item_count: item_count as u32,
        }),
        (None, None, None) => None,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                "invalid activity batch tuple".into(),
            ));
        }
    };
    Ok(ActivityRecord {
        id: row.get(0)?,
        operation,
        reason,
        outcome,
        actor,
        trigger,
        occurred_at: row.get(6)?,
        target: ActivityTarget {
            library_skill_id: row.get(7)?,
            workspace_id: row.get(8)?,
            deployment_id: row.get(9)?,
            consumer: row
                .get::<_, Option<String>>(10)?
                .and_then(|raw| match raw.as_str() {
                    "claude" => Some(crate::services::skill_deployment::DeploymentConsumer::Claude),
                    "codex" => Some(crate::services::skill_deployment::DeploymentConsumer::Codex),
                    _ => None,
                }),
            workspace_kind: row
                .get::<_, Option<String>>(11)?
                .and_then(|raw| match raw.as_str() {
                    "global" => Some(crate::services::skill_deployment::WorkspaceKind::Global),
                    "project" => Some(crate::services::skill_deployment::WorkspaceKind::Project),
                    _ => None,
                }),
        },
        batch,
        detail_code,
    })
}

const SELECT_ACTIVITY: &str = "SELECT id, operation, reason, outcome, actor, trigger,
    occurred_at, library_skill_id, workspace_id, deployment_id, consumer, workspace_kind,
    batch_id, batch_index, detail_code, batch_count
    FROM skill_activity";

impl Database {
    pub(crate) fn insert_skill_activity(
        &self,
        event: &ActivityEventInput,
        occurred_at: i64,
    ) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO skill_activity (
                operation, reason, outcome, actor, trigger, occurred_at,
                library_skill_id, workspace_id, deployment_id, consumer,
                workspace_kind, batch_id, batch_index, batch_count, detail_code
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                event.operation.as_str(),
                event.reason.as_str(),
                event.outcome.as_str(),
                event.actor.as_str(),
                event.trigger.as_str(),
                occurred_at,
                event.target.library_skill_id,
                event.target.workspace_id,
                event.target.deployment_id,
                event.target.consumer.map(|consumer| match consumer {
                    crate::services::skill_deployment::DeploymentConsumer::Claude => "claude",
                    crate::services::skill_deployment::DeploymentConsumer::Codex => "codex",
                }),
                event.target.workspace_kind.map(|kind| match kind {
                    crate::services::skill_deployment::WorkspaceKind::Global => "global",
                    crate::services::skill_deployment::WorkspaceKind::Project => "project",
                }),
                event.batch.as_ref().map(|batch| &batch.batch_id),
                event.batch.as_ref().map(|batch| batch.item_index as i64),
                event.batch.as_ref().map(|batch| batch.item_count as i64),
                event.detail_code.as_str(),
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    pub(crate) fn list_skill_activity(
        &self,
        query: &ActivityQuery,
    ) -> Result<ActivityPage, AppError> {
        let mut sql = format!("{SELECT_ACTIVITY} WHERE 1 = 1");
        let mut values: Vec<Value> = Vec::new();
        if let Some(operation) = query.operation {
            sql.push_str(" AND operation = ?");
            values.push(Value::Text(operation.as_str().to_string()));
        }
        if let Some(reason) = query.reason {
            sql.push_str(" AND reason = ?");
            values.push(Value::Text(reason.as_str().to_string()));
        }
        if let Some(outcome) = query.outcome {
            sql.push_str(" AND outcome = ?");
            values.push(Value::Text(outcome.as_str().to_string()));
        }
        for (column, value) in [
            ("library_skill_id", query.library_skill_id.as_deref()),
            ("workspace_id", query.workspace_id.as_deref()),
            ("deployment_id", query.deployment_id.as_deref()),
        ] {
            if let Some(value) = value {
                sql.push_str(&format!(" AND {column} = ?"));
                values.push(Value::Text(value.to_string()));
            }
        }
        if let Some(consumer) = query.consumer {
            sql.push_str(" AND consumer = ?");
            values.push(Value::Text(
                match consumer {
                    crate::services::skill_deployment::DeploymentConsumer::Claude => "claude",
                    crate::services::skill_deployment::DeploymentConsumer::Codex => "codex",
                }
                .to_string(),
            ));
        }
        if let Some(kind) = query.workspace_kind {
            sql.push_str(" AND workspace_kind = ?");
            values.push(Value::Text(
                match kind {
                    crate::services::skill_deployment::WorkspaceKind::Global => "global",
                    crate::services::skill_deployment::WorkspaceKind::Project => "project",
                }
                .to_string(),
            ));
        }
        if let Some(since) = query.since {
            sql.push_str(" AND occurred_at >= ?");
            values.push(Value::Integer(since));
        }
        if let Some(until) = query.until {
            sql.push_str(" AND occurred_at <= ?");
            values.push(Value::Integer(until));
        }
        if let Some(cursor) = query.cursor {
            sql.push_str(" AND (occurred_at < ? OR (occurred_at = ? AND id < ?))");
            values.push(Value::Integer(cursor.occurred_at));
            values.push(Value::Integer(cursor.occurred_at));
            values.push(Value::Integer(cursor.id));
        }

        let limit = query.normalized_limit();
        sql.push_str(" ORDER BY occurred_at DESC, id DESC LIMIT ?");
        values.push(Value::Integer(i64::from(limit) + 1));

        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&sql)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map(params_from_iter(values), decode_activity)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut entries = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let has_more = entries.len() > limit as usize;
        if has_more {
            entries.pop();
        }
        let next_cursor = has_more.then(|| entries.last()).flatten().map(|entry| {
            crate::services::activity::ActivityCursor {
                occurred_at: entry.occurred_at,
                id: entry.id,
            }
        });
        Ok(ActivityPage {
            entries,
            next_cursor,
            has_more,
        })
    }

    /// Failure injection for proving activity persistence remains subordinate
    /// to the primary domain mutation.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn fail_skill_activity_inserts_for_test(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS test_fail_skill_activity_insert;
             CREATE TRIGGER test_fail_skill_activity_insert
             BEFORE INSERT ON skill_activity
             BEGIN SELECT RAISE(ABORT, 'injected activity insert failure'); END;",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }
}
