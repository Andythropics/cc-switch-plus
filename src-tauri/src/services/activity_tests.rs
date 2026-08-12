#![cfg(target_os = "macos")]

use std::sync::Arc;

use crate::database::Database;
use crate::services::activity::{
    ActivityActor, ActivityCursor, ActivityDetailCode, ActivityEventInput, ActivityOperation,
    ActivityOutcome, ActivityQuery, ActivityReason, ActivityRecorder, ActivityTarget,
    ActivityTrigger,
};

fn event(
    operation: ActivityOperation,
    reason: ActivityReason,
    outcome: ActivityOutcome,
    library_skill_id: Option<&str>,
    workspace_id: Option<&str>,
    detail_code: ActivityDetailCode,
) -> ActivityEventInput {
    ActivityEventInput {
        operation,
        reason,
        outcome,
        actor: ActivityActor::User,
        trigger: ActivityTrigger::Command,
        target: ActivityTarget {
            library_skill_id: library_skill_id.map(str::to_owned),
            workspace_id: workspace_id.map(str::to_owned),
            deployment_id: None,
            consumer: None,
            workspace_kind: None,
        },
        batch: None,
        detail_code,
    }
}

#[test]
fn activity_records_are_ordered_and_filterable_with_keyset_cursor() -> Result<(), String> {
    let db = Arc::new(Database::memory().map_err(|error| error.to_string())?);
    let recorder = ActivityRecorder::new(db);
    recorder
        .record_at(
            event(
                ActivityOperation::Library,
                ActivityReason::Acquire,
                ActivityOutcome::Success,
                Some("skill-a"),
                None,
                ActivityDetailCode::None,
            ),
            100,
        )
        .map_err(|error| error.to_string())?;
    recorder
        .record_at(
            event(
                ActivityOperation::Workspace,
                ActivityReason::Register,
                ActivityOutcome::Blocked,
                None,
                Some("workspace-a"),
                ActivityDetailCode::TargetConflict,
            ),
            100,
        )
        .map_err(|error| error.to_string())?;
    recorder
        .record_at(
            event(
                ActivityOperation::Removal,
                ActivityReason::LibraryRemove,
                ActivityOutcome::RolledBack,
                Some("skill-a"),
                None,
                ActivityDetailCode::CompensationFailure,
            ),
            90,
        )
        .map_err(|error| error.to_string())?;

    let first = recorder
        .list(ActivityQuery {
            outcome: Some(ActivityOutcome::Blocked),
            limit: Some(1),
            ..ActivityQuery::default()
        })
        .map_err(|error| error.to_string())?;
    assert_eq!(first.entries.len(), 1);
    assert_eq!(
        first.entries[0].target.workspace_id.as_deref(),
        Some("workspace-a")
    );
    assert!(first.next_cursor.is_none());

    let filtered = recorder
        .list(ActivityQuery {
            library_skill_id: Some("skill-a".to_string()),
            operation: Some(ActivityOperation::Library),
            ..ActivityQuery::default()
        })
        .map_err(|error| error.to_string())?;
    assert_eq!(filtered.entries.len(), 1);
    assert_eq!(
        filtered.entries[0].target.library_skill_id.as_deref(),
        Some("skill-a")
    );
    Ok(())
}

#[test]
fn activity_serialization_has_no_raw_message_or_path_payload() -> Result<(), String> {
    let db = Arc::new(Database::memory().map_err(|error| error.to_string())?);
    let recorder = ActivityRecorder::new(db);
    recorder
        .record_at(
            event(
                ActivityOperation::Removal,
                ActivityReason::LibraryRemove,
                ActivityOutcome::CompensationFailed,
                Some("skill-secret"),
                Some("workspace-stable"),
                ActivityDetailCode::CompensationFailure,
            ),
            123,
        )
        .map_err(|error| error.to_string())?;
    let page = recorder
        .list(ActivityQuery::default())
        .map_err(|error| error.to_string())?;
    let encoded = serde_json::to_string(&page).map_err(|error| error.to_string())?;
    for forbidden in [
        "SKILL.md body",
        "Bearer super-secret-token",
        "/Users/example/project",
        "password=secret",
    ] {
        assert!(!encoded.contains(forbidden), "activity leaked {forbidden}");
    }
    assert!(encoded.contains("compensation_failed"));
    assert!(encoded.contains("compensation_failure"));
    Ok(())
}

#[test]
fn activity_query_rejects_unsafe_identifiers_ranges_cursors_and_zero_limit() -> Result<(), String> {
    let db = Arc::new(Database::memory().map_err(|error| error.to_string())?);
    let recorder = ActivityRecorder::new(db);
    for query in [
        ActivityQuery {
            library_skill_id: Some("../secret".to_string()),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            workspace_id: Some("workspace\0secret".to_string()),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            deployment_id: Some("x".repeat(129)),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            since: Some(20),
            until: Some(10),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            cursor: Some(ActivityCursor {
                occurred_at: -1,
                id: 1,
            }),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            cursor: Some(ActivityCursor {
                occurred_at: 1,
                id: 0,
            }),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            limit: Some(0),
            ..ActivityQuery::default()
        },
    ] {
        assert!(recorder.list(query).is_err());
    }
    Ok(())
}

#[test]
fn activity_query_applies_default_and_maximum_page_limits() -> Result<(), String> {
    let db = Arc::new(Database::memory().map_err(|error| error.to_string())?);
    let recorder = ActivityRecorder::new(db);
    for occurred_at in 1..=101 {
        recorder
            .record_at(
                event(
                    ActivityOperation::Library,
                    ActivityReason::Acquire,
                    ActivityOutcome::Success,
                    Some("skill-a"),
                    None,
                    ActivityDetailCode::None,
                ),
                occurred_at,
            )
            .map_err(|error| error.to_string())?;
    }
    let default_page = recorder
        .list(ActivityQuery::default())
        .map_err(|error| error.to_string())?;
    assert_eq!(default_page.entries.len(), 50);
    assert!(default_page.has_more);

    let capped_page = recorder
        .list(ActivityQuery {
            limit: Some(500),
            ..ActivityQuery::default()
        })
        .map_err(|error| error.to_string())?;
    assert_eq!(capped_page.entries.len(), 100);
    assert!(capped_page.has_more);
    assert!(capped_page.next_cursor.is_some());
    Ok(())
}

#[test]
fn migration_codes_are_reserved_and_cross_operation_reasons_are_rejected() -> Result<(), String> {
    let db = Arc::new(Database::memory().map_err(|error| error.to_string())?);
    let recorder = ActivityRecorder::new(db);
    let mut migration = event(
        ActivityOperation::Migration,
        ActivityReason::MigrateItem,
        ActivityOutcome::Success,
        Some("skill-a"),
        None,
        ActivityDetailCode::None,
    );
    migration.actor = ActivityActor::Migration;
    migration.trigger = ActivityTrigger::Resume;
    recorder
        .record_at(migration, 100)
        .map_err(|error| error.to_string())?;

    let invalid = event(
        ActivityOperation::Migration,
        ActivityReason::Acquire,
        ActivityOutcome::Success,
        Some("skill-a"),
        None,
        ActivityDetailCode::None,
    );
    assert!(recorder.record_at(invalid, 101).is_err());

    let page = recorder
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Migration),
            ..ActivityQuery::default()
        })
        .map_err(|error| error.to_string())?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].actor, ActivityActor::Migration);
    assert_eq!(page.entries[0].trigger, ActivityTrigger::Resume);
    Ok(())
}
