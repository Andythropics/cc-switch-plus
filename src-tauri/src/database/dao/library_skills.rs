//! Durable metadata for the redesigned private Skill Library.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::{LibrarySkill, LibrarySkillCompatibility, LibrarySkillSource};
use rusqlite::{params, OptionalExtension, Row};

fn decode_library_skill(row: &Row<'_>) -> rusqlite::Result<LibrarySkill> {
    let source_json: String = row.get(4)?;
    let compatibility_json: String = row.get(5)?;
    let source: LibrarySkillSource = serde_json::from_str(&source_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let compatibility: LibrarySkillCompatibility = serde_json::from_str(&compatibility_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;

    Ok(LibrarySkill {
        id: row.get(0)?,
        directory: row.get(1)?,
        display_name: row.get(2)?,
        description: row.get(3)?,
        source,
        compatibility,
        content_hash: row.get(6)?,
        acquired_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const SELECT_LIBRARY_SKILL: &str = "SELECT id, directory, display_name, description, source_json,
            compatibility_json, content_hash, acquired_at, updated_at
     FROM library_skills";

impl Database {
    pub fn get_library_skill_by_id(&self, id: &str) -> Result<Option<LibrarySkill>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("{SELECT_LIBRARY_SKILL} WHERE id = ?1"),
            [id],
            decode_library_skill,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn save_library_skill(&self, skill: &LibrarySkill) -> Result<(), AppError> {
        let source_json = serde_json::to_string(&skill.source)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let compatibility_json = serde_json::to_string(&skill.compatibility)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO library_skills
             (id, directory, display_name, description, source_json,
              compatibility_json, content_hash, acquired_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                skill.id,
                skill.directory,
                skill.display_name,
                skill.description,
                source_json,
                compatibility_json,
                skill.content_hash,
                skill.acquired_at,
                skill.updated_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn list_library_skills(&self) -> Result<Vec<LibrarySkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut statement = conn
            .prepare(&format!(
                "{SELECT_LIBRARY_SKILL} ORDER BY display_name COLLATE NOCASE, directory COLLATE NOCASE"
            ))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], decode_library_skill)
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn get_library_skill_by_directory(
        &self,
        directory: &str,
    ) -> Result<Option<LibrarySkill>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("{SELECT_LIBRARY_SKILL} WHERE directory = ?1 COLLATE NOCASE"),
            [directory],
            decode_library_skill,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn get_library_skill_by_content_hash(
        &self,
        content_hash: &str,
    ) -> Result<Option<LibrarySkill>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("{SELECT_LIBRARY_SKILL} WHERE content_hash = ?1"),
            [content_hash],
            decode_library_skill,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn update_library_skill_display_metadata(
        &self,
        id: &str,
        display_name: &str,
        description: Option<&str>,
        updated_at: i64,
    ) -> Result<Option<LibrarySkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute(
                "UPDATE library_skills
                 SET display_name = ?1, description = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![display_name, description, updated_at, id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if changed == 0 {
            return Ok(None);
        }
        conn.query_row(
            &format!("{SELECT_LIBRARY_SKILL} WHERE id = ?1"),
            [id],
            decode_library_skill,
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))
    }
}
