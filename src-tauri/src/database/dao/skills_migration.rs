//! Durable journal access for the guided Skills migration.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension, Row};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationRunRecord {
    pub id: String,
    pub accepted_observation_token: String,
    pub resume_token: String,
    pub state: String,
    pub database_backup_filename: Option<String>,
    pub content_backup_root: Option<String>,
    pub plan_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationItemRecord {
    pub run_id: String,
    pub ordinal: u32,
    pub item_key: String,
    pub action: String,
    pub directory: Option<String>,
    pub consumer: Option<String>,
    pub source_location: Option<String>,
    pub target_location: Option<String>,
    pub expected_fingerprint: Option<String>,
    pub state: String,
    pub library_skill_id: Option<String>,
    pub detail_code: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

pub(crate) struct SkillsMigrationItemUpdate<'a> {
    pub state: &'a str,
    pub library_skill_id: Option<&'a str>,
    pub detail_code: Option<&'a str>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

fn decode_run(row: &Row<'_>) -> rusqlite::Result<SkillsMigrationRunRecord> {
    Ok(SkillsMigrationRunRecord {
        id: row.get(0)?,
        accepted_observation_token: row.get(1)?,
        resume_token: row.get(2)?,
        state: row.get(3)?,
        database_backup_filename: row.get(4)?,
        content_backup_root: row.get(5)?,
        plan_hash: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        completed_at: row.get(9)?,
    })
}

fn decode_item(row: &Row<'_>) -> rusqlite::Result<SkillsMigrationItemRecord> {
    Ok(SkillsMigrationItemRecord {
        run_id: row.get(0)?,
        ordinal: row.get::<_, i64>(1)? as u32,
        item_key: row.get(2)?,
        action: row.get(3)?,
        directory: row.get(4)?,
        consumer: row.get(5)?,
        source_location: row.get(6)?,
        target_location: row.get(7)?,
        expected_fingerprint: row.get(8)?,
        state: row.get(9)?,
        library_skill_id: row.get(10)?,
        detail_code: row.get(11)?,
        started_at: row.get(12)?,
        completed_at: row.get(13)?,
    })
}

const SELECT_RUN: &str = "SELECT id, accepted_observation_token, resume_token, state,
    database_backup_filename, content_backup_root, plan_hash, created_at, updated_at, completed_at
    FROM skills_migration_runs";
const SELECT_ITEM: &str = "SELECT run_id, ordinal, item_key, action, directory, consumer,
    source_location, target_location, expected_fingerprint, state, library_skill_id, detail_code,
    started_at, completed_at FROM skills_migration_items";

impl Database {
    pub(crate) fn insert_skills_migration_run_with_items(
        &self,
        run: &SkillsMigrationRunRecord,
        items: &[SkillsMigrationItemRecord],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO skills_migration_runs (
                    id, accepted_observation_token, resume_token, state,
                    database_backup_filename, content_backup_root, plan_hash,
                    created_at, updated_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    run.id,
                    run.accepted_observation_token,
                    run.resume_token,
                    run.state,
                    run.database_backup_filename,
                    run.content_backup_root,
                    run.plan_hash,
                    run.created_at,
                    run.updated_at,
                    run.completed_at,
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        for item in items {
            transaction
                .execute(
                    "INSERT INTO skills_migration_items (
                        run_id, ordinal, item_key, action, directory, consumer,
                        source_location, target_location, expected_fingerprint, state,
                        library_skill_id, detail_code, started_at, completed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        item.run_id,
                        i64::from(item.ordinal),
                        item.item_key,
                        item.action,
                        item.directory,
                        item.consumer,
                        item.source_location,
                        item.target_location,
                        item.expected_fingerprint,
                        item.state,
                        item.library_skill_id,
                        item.detail_code,
                        item.started_at,
                        item.completed_at,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        transaction
            .commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn replace_skills_migration_run_with_items(
        &self,
        run: &SkillsMigrationRunRecord,
        items: &[SkillsMigrationItemRecord],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .execute("DELETE FROM skills_migration_runs WHERE id = ?1", [&run.id])
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO skills_migration_runs (
                    id, accepted_observation_token, resume_token, state,
                    database_backup_filename, content_backup_root, plan_hash,
                    created_at, updated_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    run.id,
                    run.accepted_observation_token,
                    run.resume_token,
                    run.state,
                    run.database_backup_filename,
                    run.content_backup_root,
                    run.plan_hash,
                    run.created_at,
                    run.updated_at,
                    run.completed_at,
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        for item in items {
            transaction
                .execute(
                    "INSERT INTO skills_migration_items (
                        run_id, ordinal, item_key, action, directory, consumer,
                        source_location, target_location, expected_fingerprint, state,
                        library_skill_id, detail_code, started_at, completed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        item.run_id,
                        i64::from(item.ordinal),
                        item.item_key,
                        item.action,
                        item.directory,
                        item.consumer,
                        item.source_location,
                        item.target_location,
                        item.expected_fingerprint,
                        item.state,
                        item.library_skill_id,
                        item.detail_code,
                        item.started_at,
                        item.completed_at,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        transaction
            .commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn get_active_skills_migration_run(
        &self,
    ) -> Result<Option<SkillsMigrationRunRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!(
                "{SELECT_RUN} WHERE state IN ('prepared', 'running', 'blocked', 'recovery_required')
                 ORDER BY updated_at DESC, id DESC LIMIT 1"
            ),
            [],
            decode_run,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn get_skills_migration_run_by_backup_id(
        &self,
        backup_id: &str,
    ) -> Result<Option<SkillsMigrationRunRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("{SELECT_RUN} WHERE id = ?1"),
            [backup_id],
            decode_run,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn delete_skills_migration_run(&self, run_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM skills_migration_runs WHERE id = ?1", [run_id])
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub(crate) fn list_skills_migration_items(
        &self,
        run_id: &str,
    ) -> Result<Vec<SkillsMigrationItemRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&format!("{SELECT_ITEM} WHERE run_id = ?1 ORDER BY ordinal"))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([run_id], decode_item)
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn update_skills_migration_run(
        &self,
        run_id: &str,
        state: &str,
        database_backup_filename: Option<&str>,
        content_backup_root: Option<&str>,
        updated_at: i64,
        completed_at: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE skills_migration_runs
             SET state = ?1,
                 database_backup_filename = COALESCE(?2, database_backup_filename),
                 content_backup_root = COALESCE(?3, content_backup_root),
                 updated_at = ?4,
                 completed_at = ?5
             WHERE id = ?6",
            params![
                state,
                database_backup_filename,
                content_backup_root,
                updated_at,
                completed_at,
                run_id
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub(crate) fn update_skills_migration_item(
        &self,
        run_id: &str,
        ordinal: u32,
        update: SkillsMigrationItemUpdate<'_>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE skills_migration_items
             SET state = ?1,
                 library_skill_id = COALESCE(?2, library_skill_id),
                 detail_code = ?3,
                 started_at = COALESCE(?4, started_at),
                 completed_at = ?5
             WHERE run_id = ?6 AND ordinal = ?7",
            params![
                update.state,
                update.library_skill_id,
                update.detail_code,
                update.started_at,
                update.completed_at,
                run_id,
                i64::from(ordinal)
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub(crate) fn clear_legacy_skills_migration_state(&self) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .execute("DELETE FROM skills", [])
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .execute(
                "DELETE FROM settings WHERE key IN (
                    'skills_ssot_migration_pending', 'skills_ssot_migration_snapshot'
                 )",
                [],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        transaction
            .commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }
}
