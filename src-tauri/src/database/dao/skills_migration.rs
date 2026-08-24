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

/// The accepted preflight is kept separately from the execution journal so a
/// completed migration can still explain what was observed at Apply time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationPreflightSnapshotRecord {
    pub run_id: String,
    pub version: i64,
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationReportMetadataRecord {
    pub run_id: String,
    pub accepted_preflight_snapshot_version: Option<i64>,
    pub accepted_preflight_snapshot: Option<String>,
    pub report_seen_at: Option<i64>,
    pub report_acknowledged_at: Option<i64>,
}

/// A durable copy of one preflight item.  `finding_key` is stable within a
/// run and is the idempotency key used by the backfill/upsert path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationFindingRecord {
    pub run_id: String,
    pub finding_key: String,
    pub disposition: String,
    pub action: String,
    pub reason: String,
    pub directory: Option<String>,
    pub consumer: Option<String>,
    pub source_location: Option<String>,
    pub target_location: Option<String>,
    pub observed_location: Option<String>,
    /// JSON text because some legacy consumers (for example Hermes) are not
    /// valid values for the execution journal's constrained `consumer` field.
    /// The underlying column is named `consumer_codes`; this field keeps the
    /// preflight vocabulary used by the service layer.
    pub unsupported_consumers_json: String,
    pub status: String,
    pub detail_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsMigrationReportRecord {
    pub run: SkillsMigrationRunRecord,
    pub metadata: SkillsMigrationReportMetadataRecord,
    pub findings: Vec<SkillsMigrationFindingRecord>,
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
const SELECT_FINDING: &str = "SELECT run_id, finding_key, disposition, action, reason,
    directory, consumer, source_location, target_location, observed_location,
    consumer_codes, status, detail_code, created_at, updated_at
    FROM skills_migration_findings";

fn decode_snapshot(row: &Row<'_>) -> rusqlite::Result<SkillsMigrationPreflightSnapshotRecord> {
    Ok(SkillsMigrationPreflightSnapshotRecord {
        run_id: row.get(0)?,
        version: row.get(1)?,
        snapshot: row.get(2)?,
    })
}

fn decode_report_metadata(row: &Row<'_>) -> rusqlite::Result<SkillsMigrationReportMetadataRecord> {
    Ok(SkillsMigrationReportMetadataRecord {
        run_id: row.get(0)?,
        accepted_preflight_snapshot_version: row.get(1)?,
        accepted_preflight_snapshot: row.get(2)?,
        report_seen_at: row.get(3)?,
        report_acknowledged_at: row.get(4)?,
    })
}

fn decode_finding(row: &Row<'_>) -> rusqlite::Result<SkillsMigrationFindingRecord> {
    Ok(SkillsMigrationFindingRecord {
        run_id: row.get(0)?,
        finding_key: row.get(1)?,
        disposition: row.get(2)?,
        action: row.get(3)?,
        reason: row.get(4)?,
        directory: row.get(5)?,
        consumer: row.get(6)?,
        source_location: row.get(7)?,
        target_location: row.get(8)?,
        observed_location: row.get(9)?,
        unsupported_consumers_json: row.get(10)?,
        status: row.get(11)?,
        detail_code: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn report_run_not_found_or_incomplete(conn: &rusqlite::Connection, run_id: &str) -> AppError {
    match conn
        .query_row(
            "SELECT state FROM skills_migration_runs WHERE id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    {
        Ok(None) => AppError::Database(format!("Skills migration report not found: {run_id}")),
        Ok(Some(state)) => AppError::Database(format!(
            "Skills migration report is not complete: {run_id} ({state})"
        )),
        Err(error) => AppError::Database(error.to_string()),
    }
}

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
            // Keep the report columns and findings attached to the run. The
            // old implementation deleted the parent row first, which was
            // harmless before findings existed but now triggers ON DELETE
            // CASCADE and loses aftercare evidence during restore.
            .execute(
                "DELETE FROM skills_migration_items WHERE run_id = ?1",
                [&run.id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let updated = transaction
            .execute(
                "UPDATE skills_migration_runs
                 SET accepted_observation_token = ?1,
                     resume_token = ?2,
                     state = ?3,
                     database_backup_filename = ?4,
                     content_backup_root = ?5,
                     plan_hash = ?6,
                     created_at = ?7,
                     updated_at = ?8,
                     completed_at = ?9
                 WHERE id = ?10",
                params![
                    run.accepted_observation_token,
                    run.resume_token,
                    run.state,
                    run.database_backup_filename,
                    run.content_backup_root,
                    run.plan_hash,
                    run.created_at,
                    run.updated_at,
                    run.completed_at,
                    run.id,
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if updated == 0 {
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
        }
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

    /// Return the most recent completed migration report.  Restored runs are
    /// included because restoring the filesystem is still a durable outcome
    /// worth showing in migration history.
    pub(crate) fn get_latest_completed_or_restored_skills_migration_run(
        &self,
    ) -> Result<Option<SkillsMigrationRunRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!(
                "{SELECT_RUN}
                 WHERE state IN ('completed', 'restored')
                 ORDER BY COALESCE(completed_at, updated_at) DESC, updated_at DESC, id DESC
                 LIMIT 1"
            ),
            [],
            decode_run,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn get_skills_migration_report_metadata(
        &self,
        run_id: &str,
    ) -> Result<Option<SkillsMigrationReportMetadataRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, accepted_preflight_snapshot_version,
                    accepted_preflight_snapshot, report_seen_at,
                    report_acknowledged_at
             FROM skills_migration_runs WHERE id = ?1",
            [run_id],
            decode_report_metadata,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    /// Persist an accepted preflight exactly once.  Replaying the same value
    /// is idempotent; changing an accepted snapshot is rejected so a report
    /// can never silently describe a different Apply decision.
    pub(crate) fn save_skills_migration_preflight_snapshot(
        &self,
        run_id: &str,
        version: i64,
        snapshot: &str,
    ) -> Result<(), AppError> {
        if version <= 0 {
            return Err(AppError::InvalidInput(
                "Skills migration preflight snapshot version must be positive".to_string(),
            ));
        }
        if snapshot.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "Skills migration preflight snapshot must not be empty".to_string(),
            ));
        }

        let conn = lock_conn!(self.conn);
        let updated = conn
            .execute(
                "UPDATE skills_migration_runs
                 SET accepted_preflight_snapshot_version = ?1,
                     accepted_preflight_snapshot = ?2
                 WHERE id = ?3
                   AND ((accepted_preflight_snapshot_version IS NULL
                         AND accepted_preflight_snapshot IS NULL)
                        OR (accepted_preflight_snapshot_version = ?1
                            AND accepted_preflight_snapshot = ?2))",
                params![version, snapshot, run_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if updated == 0 {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM skills_migration_runs WHERE id = ?1)",
                    [run_id],
                    |row| row.get(0),
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
            if !exists {
                return Err(AppError::Database(format!(
                    "Skills migration run not found: {run_id}"
                )));
            }
            return Err(AppError::Database(format!(
                "Accepted Skills migration preflight snapshot is immutable: {run_id}"
            )));
        }
        Ok(())
    }

    pub(crate) fn get_skills_migration_preflight_snapshot(
        &self,
        run_id: &str,
    ) -> Result<Option<SkillsMigrationPreflightSnapshotRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, accepted_preflight_snapshot_version,
                    accepted_preflight_snapshot
             FROM skills_migration_runs
             WHERE id = ?1
               AND accepted_preflight_snapshot_version IS NOT NULL
               AND accepted_preflight_snapshot IS NOT NULL",
            [run_id],
            decode_snapshot,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn mark_skills_migration_report_seen(
        &self,
        run_id: &str,
        seen_at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let updated = conn
            .execute(
                "UPDATE skills_migration_runs
             SET report_seen_at = COALESCE(report_seen_at, ?1)
             WHERE id = ?2 AND state IN ('completed', 'restored')",
                params![seen_at, run_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(report_run_not_found_or_incomplete(&conn, run_id));
        }
        Ok(())
    }

    /// Acknowledgement is idempotent and also records the first time the
    /// report became visible to the user when no separate seen event exists.
    pub(crate) fn acknowledge_skills_migration_report(
        &self,
        run_id: &str,
        acknowledged_at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let updated = conn
            .execute(
                "UPDATE skills_migration_runs
             SET report_seen_at = COALESCE(report_seen_at, ?1),
                 report_acknowledged_at = COALESCE(report_acknowledged_at, ?1)
             WHERE id = ?2 AND state IN ('completed', 'restored')",
                params![acknowledged_at, run_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(report_run_not_found_or_incomplete(&conn, run_id));
        }
        Ok(())
    }

    pub(crate) fn list_skills_migration_findings(
        &self,
        run_id: &str,
    ) -> Result<Vec<SkillsMigrationFindingRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&format!(
                "{SELECT_FINDING} WHERE run_id = ?1 ORDER BY finding_key"
            ))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([run_id], decode_finding)
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    /// Insert or update a batch of findings in one transaction.  This is the
    /// persistence seam used by both the initial Apply and legacy backfill.
    pub(crate) fn upsert_skills_migration_findings(
        &self,
        findings: &[SkillsMigrationFindingRecord],
    ) -> Result<(), AppError> {
        if findings.is_empty() {
            return Ok(());
        }

        let mut conn = lock_conn!(self.conn);
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        for finding in findings {
            transaction
                .execute(
                    "INSERT INTO skills_migration_findings (
                        run_id, finding_key, disposition, action, reason,
                        directory, consumer, source_location, target_location,
                        observed_location, consumer_codes, status,
                        detail_code, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                               ?11, ?12, ?13, ?14, ?15)
                     ON CONFLICT(run_id, finding_key) DO UPDATE SET
                        disposition = excluded.disposition,
                        action = excluded.action,
                        reason = excluded.reason,
                        directory = excluded.directory,
                        consumer = excluded.consumer,
                        source_location = excluded.source_location,
                        target_location = excluded.target_location,
                        observed_location = excluded.observed_location,
                        consumer_codes = excluded.consumer_codes,
                        status = CASE
                            WHEN skills_migration_findings.status IN (
                                'resolved', 'acknowledged', 'dismissed',
                                'preserved', 'preserved_with_consent'
                            ) THEN skills_migration_findings.status
                            ELSE excluded.status
                        END,
                        detail_code = excluded.detail_code,
                        updated_at = excluded.updated_at",
                    params![
                        finding.run_id,
                        finding.finding_key,
                        finding.disposition,
                        finding.action,
                        finding.reason,
                        finding.directory,
                        finding.consumer,
                        finding.source_location,
                        finding.target_location,
                        finding.observed_location,
                        finding.unsupported_consumers_json,
                        finding.status,
                        finding.detail_code,
                        finding.created_at,
                        finding.updated_at,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        transaction
            .commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn get_skills_migration_report(
        &self,
        run_id: &str,
    ) -> Result<Option<SkillsMigrationReportRecord>, AppError> {
        let Some(run) = self.get_skills_migration_run_by_backup_id(run_id)? else {
            return Ok(None);
        };
        let metadata = self
            .get_skills_migration_report_metadata(run_id)?
            .ok_or_else(|| {
                AppError::Database(format!("Skills migration run not found: {run_id}"))
            })?;
        let findings = self.list_skills_migration_findings(run_id)?;
        Ok(Some(SkillsMigrationReportRecord {
            run,
            metadata,
            findings,
        }))
    }

    pub(crate) fn get_latest_skills_migration_report(
        &self,
    ) -> Result<Option<SkillsMigrationReportRecord>, AppError> {
        let Some(run) = self.get_latest_completed_or_restored_skills_migration_run()? else {
            return Ok(None);
        };
        let metadata = self
            .get_skills_migration_report_metadata(&run.id)?
            .ok_or_else(|| {
                AppError::Database(format!("Skills migration run not found: {}", run.id))
            })?;
        let findings = self.list_skills_migration_findings(&run.id)?;
        Ok(Some(SkillsMigrationReportRecord {
            run,
            metadata,
            findings,
        }))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_run(id: &str, updated_at: i64) -> SkillsMigrationRunRecord {
        SkillsMigrationRunRecord {
            id: id.to_string(),
            accepted_observation_token: format!("observation-{id}"),
            resume_token: format!("resume-{id}"),
            state: "completed".to_string(),
            database_backup_filename: None,
            content_backup_root: None,
            plan_hash: format!("plan-{id}"),
            created_at: updated_at,
            updated_at,
            completed_at: Some(updated_at),
        }
    }

    #[test]
    fn migration_report_snapshot_findings_and_ack_are_durable_and_idempotent(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let run = completed_run("report-run", 20);
        db.insert_skills_migration_run_with_items(&run, &[])?;

        db.save_skills_migration_preflight_snapshot(&run.id, 1, r#"{"status":"not_required"}"#)?;
        db.save_skills_migration_preflight_snapshot(&run.id, 1, r#"{"status":"not_required"}"#)?;
        let snapshot = db
            .get_skills_migration_preflight_snapshot(&run.id)?
            .expect("accepted snapshot");
        assert_eq!(
            snapshot,
            SkillsMigrationPreflightSnapshotRecord {
                run_id: run.id.clone(),
                version: 1,
                snapshot: r#"{"status":"not_required"}"#.to_string(),
            }
        );
        assert!(db
            .save_skills_migration_preflight_snapshot(&run.id, 2, r#"{"status":"blocked"}"#)
            .is_err());

        let first = SkillsMigrationFindingRecord {
            run_id: run.id.clone(),
            finding_key: "hermes/computer-use".to_string(),
            disposition: "preserve_with_consent".to_string(),
            action: "preserve_unsupported_consumer_files".to_string(),
            reason: "unsupported_consumer_enabled".to_string(),
            directory: Some("computer-use".to_string()),
            consumer: Some("hermes".to_string()),
            source_location: Some("/legacy/hermes/computer-use".to_string()),
            target_location: None,
            observed_location: Some("/legacy/hermes/computer-use".to_string()),
            unsupported_consumers_json: r#"["hermes"]"#.to_string(),
            status: "open".to_string(),
            detail_code: Some("preserved_with_consent".to_string()),
            created_at: 30,
            updated_at: 30,
        };
        db.upsert_skills_migration_findings(std::slice::from_ref(&first))?;

        let mut updated = first.clone();
        updated.status = "acknowledged".to_string();
        updated.updated_at = 40;
        let second = SkillsMigrationFindingRecord {
            run_id: run.id.clone(),
            finding_key: "hermes/dogfood".to_string(),
            directory: Some("dogfood".to_string()),
            unsupported_consumers_json: r#"["hermes"]"#.to_string(),
            created_at: 31,
            updated_at: 31,
            ..first.clone()
        };
        db.upsert_skills_migration_findings(&[updated.clone(), second])?;

        // A later lazy backfill may carry a stale status, but must not erase a
        // terminal user state already recorded for this finding.
        let mut stale = first.clone();
        stale.status = "incomplete".to_string();
        stale.updated_at = 45;
        db.upsert_skills_migration_findings(std::slice::from_ref(&stale))?;

        let findings = db.list_skills_migration_findings(&run.id)?;
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].finding_key, "hermes/computer-use");
        assert_eq!(findings[0].status, "acknowledged");
        assert_eq!(findings[0].created_at, 30);
        assert_eq!(findings[0].consumer, Some("hermes".to_string()));

        db.mark_skills_migration_report_seen(&run.id, 50)?;
        db.mark_skills_migration_report_seen(&run.id, 55)?;
        db.acknowledge_skills_migration_report(&run.id, 60)?;
        // Both report lifecycle operations are idempotent.  A later duplicate
        // acknowledgement cannot erase the first audit timestamp.
        db.acknowledge_skills_migration_report(&run.id, 70)?;
        let metadata = db
            .get_skills_migration_report_metadata(&run.id)?
            .expect("report metadata");
        assert_eq!(metadata.report_seen_at, Some(50));
        assert_eq!(metadata.report_acknowledged_at, Some(60));

        let mut pending = completed_run("pending-run", 80);
        pending.state = "running".to_string();
        pending.completed_at = None;
        db.insert_skills_migration_run_with_items(&pending, &[])?;
        assert!(db
            .mark_skills_migration_report_seen(&pending.id, 90)
            .is_err());
        assert!(db
            .acknowledge_skills_migration_report(&pending.id, 90)
            .is_err());
        assert!(db
            .mark_skills_migration_report_seen("missing-run", 90)
            .is_err());

        let mut restored = run.clone();
        restored.state = "restored".to_string();
        restored.updated_at = 100;
        restored.completed_at = Some(100);
        db.replace_skills_migration_run_with_items(&restored, &[])?;
        assert!(db
            .get_skills_migration_preflight_snapshot(&run.id)?
            .is_some());
        assert_eq!(db.list_skills_migration_findings(&run.id)?.len(), 2);
        assert_eq!(
            db.get_skills_migration_report_metadata(&run.id)?
                .expect("restored report metadata")
                .report_acknowledged_at,
            Some(60)
        );

        let report = db
            .get_latest_skills_migration_report()?
            .expect("latest report");
        assert_eq!(report.run.id, run.id);
        assert_eq!(report.run.state, "restored");
        assert_eq!(report.metadata.accepted_preflight_snapshot_version, Some(1));
        assert_eq!(report.findings.len(), 2);
        Ok(())
    }
}
