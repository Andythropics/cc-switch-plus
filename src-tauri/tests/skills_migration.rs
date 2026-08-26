#![cfg(target_os = "macos")]

use std::fs;

use rusqlite::Connection;

use cc_switch_lib::{
    ActivityOperation, ActivityOutcome, ActivityQuery, ActivityReason, ActivityRecorder,
    DeploymentConsumer, DeploymentQuery, DeploymentStatus, DeploymentTarget, InstalledSkill,
    SkillApps, SkillDeploymentService, SkillsMigrationAction, SkillsMigrationExecutionOutcome,
    SkillsMigrationExecutionService, SkillsMigrationFindingRevealIntent, SkillsMigrationIntent,
    SkillsMigrationItemOutcome, SkillsMigrationPageMode, SkillsMigrationPreviewService,
    SkillsMigrationReportAckIntent, SkillsMigrationRestoreIntent,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(directory: &std::path::Path, name: &str) {
    fs::create_dir_all(directory).expect("create Skill directory");
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: migration fixture\n---\n"),
    )
    .expect("write SKILL.md");
}

fn installed_skill(directory: &str, claude: bool, codex: bool) -> InstalledSkill {
    InstalledSkill {
        id: format!("legacy:{directory}"),
        name: directory.to_string(),
        description: None,
        directory: directory.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps {
            claude,
            codex,
            ..SkillApps::default()
        },
        installed_at: 1,
        content_hash: None,
        updated_at: 0,
    }
}

fn seed_snapshot(state: &cc_switch_lib::AppState, rows: &str) {
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending migration");
    state
        .db
        .set_setting("skills_ssot_migration_snapshot", rows)
        .expect("seed legacy evidence");
}

#[test]
fn stale_observation_does_not_create_a_journal_backup_or_filesystem_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending migration");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        )
        .expect("seed legacy evidence");
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    fs::write(
        home.join(".claude/skills/review/SKILL.md"),
        "---\nname: changed\ndescription: changed after review\n---\n",
    )
    .expect("change observed source");

    let result = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("staleness is a typed outcome");

    assert_eq!(
        result.outcome,
        SkillsMigrationExecutionOutcome::StaleObservation
    );
    assert_eq!(result.page_mode, SkillsMigrationPageMode::ReadOnly);
    assert_eq!(result.progress.completed_items, 0);
    assert_eq!(result.progress.total_items, 0);
    assert!(result.items.is_empty());
    assert!(result.backup.is_none());
    assert!(!home.join(".cc-switch/skills").exists());
    assert!(!home.join(".cc-switch/skills-migration-backups").exists());
    let reinspected = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("reinspect after stale apply");
    assert!(reinspected.execution.is_none());
}

#[test]
fn migration_moves_managed_content_deploys_enabled_consumers_and_preserves_unmanaged_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", true, true))
        .expect("seed managed legacy Skill");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    for link in [
        home.join(".claude/skills/review"),
        home.join(".codex/skills/review"),
    ] {
        fs::create_dir_all(link.parent().unwrap()).expect("create legacy consumer root");
        std::os::unix::fs::symlink(home.join(".cc-switch/skills/review"), link)
            .expect("create proven managed link");
    }
    write_skill(&home.join(".agents/skills/user-owned"), "user-owned");
    let unmanaged_before = fs::read(home.join(".agents/skills/user-owned/SKILL.md")).unwrap();
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");

    let result = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("apply migration");

    assert_eq!(result.outcome, SkillsMigrationExecutionOutcome::Completed);
    assert_eq!(result.page_mode, SkillsMigrationPageMode::Writable);
    assert_eq!(result.progress.completed_items, result.progress.total_items);
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
    assert!(home.join(".cc-switch/skills/review").is_dir());
    for (path, expected) in [
        (
            home.join(".claude/skills/review"),
            DeploymentConsumer::Claude,
        ),
        (
            home.join(".agents/skills/review"),
            DeploymentConsumer::Codex,
        ),
    ] {
        assert!(path.is_symlink());
        assert_eq!(
            fs::read_link(&path).unwrap(),
            home.join(".cc-switch/skills/review")
        );
        let inspected = SkillDeploymentService::new(state.db.clone())
            .inspect(DeploymentQuery::for_target(
                cc_switch_lib::DeploymentTarget::global(expected),
            ))
            .unwrap();
        assert_eq!(
            inspected
                .items
                .iter()
                .find(|item| item.library_directory == "review")
                .unwrap()
                .status,
            DeploymentStatus::InSync
        );
    }
    assert!(!home.join(".codex/skills/review").exists());
    assert_eq!(
        fs::read(home.join(".agents/skills/user-owned/SKILL.md")).unwrap(),
        unmanaged_before
    );
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
    let after = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect completed migration");
    assert_eq!(after.page_mode, SkillsMigrationPageMode::Writable);
    let migration_activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Migration),
            ..ActivityQuery::default()
        })
        .unwrap();
    assert!(migration_activity
        .entries
        .iter()
        .any(|entry| entry.reason == ActivityReason::MigrateItem));
    assert_eq!(
        migration_activity
            .entries
            .iter()
            .filter(|entry| entry.reason == ActivityReason::Migrate)
            .count(),
        1
    );
    let repeated = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: after.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("repeat completed migration");
    assert_eq!(repeated.outcome, SkillsMigrationExecutionOutcome::Completed);
    assert_eq!(repeated.page_mode, SkillsMigrationPageMode::Writable);
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
}

#[test]
fn unified_official_codex_link_is_adopted_without_being_scheduled_for_cleanup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let settings = cc_switch_lib::AppSettings {
        skill_storage_location: cc_switch_lib::SkillStorageLocation::Unified,
        ..Default::default()
    };
    cc_switch_lib::update_settings(settings).expect("select unified legacy SSOT");
    let state = create_test_state().expect("create test state");
    let seed = home.join("seed/review");
    write_skill(&seed, "review");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &seed,
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::LocalImport,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("review"),
    )
    .expect("seed private Library Skill");
    state
        .db
        .save_skill(&installed_skill("review", false, true))
        .expect("seed enabled legacy Codex row");
    let official = home.join(".agents/skills/review");
    fs::create_dir_all(official.parent().unwrap()).expect("create official Codex root");
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/review"), &official)
        .expect("create existing official link");

    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect unified migration");
    assert!(preview.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::CreateGlobalDeployment
            && item.consumer == Some(DeploymentConsumer::Codex)
    }));
    assert!(!preview.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::RemoveLegacyCodexLink
            && item.from_location.as_deref() == Some(official.to_string_lossy().as_ref())
    }));

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("apply unified migration");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(official.is_symlink());
    assert_eq!(
        fs::read_link(&official).unwrap(),
        home.join(".cc-switch/skills/review")
    );
    let inspected = SkillDeploymentService::new(state.db.clone())
        .inspect(DeploymentQuery::for_target(DeploymentTarget::global(
            DeploymentConsumer::Codex,
        )))
        .expect("inspect adopted Codex deployment");
    assert!(inspected.items.iter().any(|item| {
        item.library_directory == "review" && item.status == DeploymentStatus::InSync
    }));
}

#[test]
fn interrupted_migration_resumes_without_repeating_completed_items() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    for interrupted_action in [
        SkillsMigrationAction::MoveToLibrary,
        SkillsMigrationAction::CreateGlobalDeployment,
        SkillsMigrationAction::Finalize,
    ] {
        reset_test_fs();
        let home = ensure_test_home();
        let state = create_test_state().expect("create test state");
        seed_snapshot(
            &state,
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        );
        write_skill(&home.join(".claude/skills/review"), "review");
        let preview = SkillsMigrationPreviewService::new(state.db.clone())
            .inspect()
            .expect("inspect migration");
        SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(interrupted_action));
        let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
            .start(SkillsMigrationIntent {
                observation_token: preview.observation_token,
                preserve_unsupported_consumer_files: false,
            })
            .expect("interrupt migration");
        assert_eq!(
            interrupted.outcome,
            SkillsMigrationExecutionOutcome::Resumable
        );

        let resumed = SkillsMigrationExecutionService::new(state.db.clone())
            .resume()
            .expect("resume migration");
        assert_eq!(
            resumed.outcome,
            SkillsMigrationExecutionOutcome::Completed,
            "resume after {interrupted_action:?} returned {resumed:#?}"
        );
        assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
        if interrupted_action != SkillsMigrationAction::MoveToLibrary {
            assert!(resumed.items.iter().any(|item| {
                item.action == SkillsMigrationAction::MoveToLibrary
                    && matches!(
                        item.outcome,
                        cc_switch_lib::SkillsMigrationItemOutcome::AlreadyCompleted
                    )
            }));
        }
    }
}

#[test]
fn resume_blocks_when_a_backed_up_source_changes_before_admission() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let source = home.join(".claude/skills/review");
    write_skill(&source, "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("interrupt after verified backup");
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    fs::write(
        source.join("SKILL.md"),
        "---\nname: changed\ndescription: changed after backup\n---\n",
    )
    .unwrap();

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("source drift is typed");

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(state.db.list_library_skills().unwrap().is_empty());
    assert!(fs::read_to_string(source.join("SKILL.md"))
        .unwrap()
        .contains("changed after backup"));
}

#[test]
fn resume_reconciles_a_completed_mutation_after_journal_completion_failure() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    SkillsMigrationExecutionService::fail_journal_completion_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("journal completion failure is resumable");
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("resume reconciles the completed filesystem mutation");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
    assert!(home.join(".claude/skills/review").is_symlink());
}

#[test]
fn resume_cleans_a_journaled_source_staging_after_retire_interruption() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let source = home.join(".claude/skills/review");
    write_skill(&source, "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    SkillsMigrationExecutionService::interrupt_after_source_retire_for_test(true);

    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("retire interruption remains resumable");
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    assert!(!source.exists());
    assert!(fs::read_dir(source.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".cc-switch-migrated-review-")));

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("resume removes deterministic staging");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(source.is_symlink());
    assert!(!fs::read_dir(source.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".cc-switch-migrated-review-")));
}

#[test]
fn blocked_skill_does_not_prevent_an_independent_skill_from_completing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[
            {"directory":"alpha","app_type":"claude","installed":true},
            {"directory":"beta","app_type":"claude","installed":true}
        ]"#,
    );
    write_skill(&home.join(".claude/skills/alpha"), "alpha");
    write_skill(&home.join(".claude/skills/beta"), "beta");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("interrupt before first item");
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    fs::write(
        home.join(".claude/skills/alpha/SKILL.md"),
        "---\nname: alpha changed\ndescription: changed after backup\n---\n",
    )
    .expect("change only the first source");

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("independent items continue after a blocked item");

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(state
        .db
        .get_library_skill_by_directory("alpha")
        .unwrap()
        .is_none());
    assert!(state
        .db
        .get_library_skill_by_directory("beta")
        .unwrap()
        .is_some());
    assert!(home.join(".claude/skills/beta").is_symlink());
    assert!(state
        .db
        .get_setting("skills_ssot_migration_snapshot")
        .unwrap()
        .is_some());
}

#[test]
fn disabled_managed_skill_moves_to_library_without_creating_deployments() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", false, false))
        .expect("seed disabled managed Skill");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect disabled managed Skill");

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("migrate disabled managed Skill");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(state
        .db
        .get_library_skill_by_directory("review")
        .unwrap()
        .is_some());
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
    assert!(!home.join(".claude/skills/review").exists());
    assert!(!home.join(".agents/skills/review").exists());
}

#[test]
fn unsupported_enabled_consumer_requires_consent_and_preserves_external_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let mut legacy = installed_skill("review", false, false);
    legacy.apps.gemini = true;
    state.db.save_skill(&legacy).expect("seed legacy Skill row");
    let source = home.join(".cc-switch/skills/review");
    write_skill(&source, "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token.clone(),
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(blocked.backup.is_none());
    assert!(source.is_dir());
    assert!(state.db.list_library_skills().unwrap().is_empty());
    assert_eq!(state.db.get_all_installed_skills().unwrap().len(), 1);

    let hermes_file = home.join(".hermes/skills/review/keep.txt");
    fs::create_dir_all(hermes_file.parent().unwrap()).expect("create Hermes deployment");
    fs::write(&hermes_file, "external Hermes state").expect("write Hermes deployment");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: true,
        })
        .expect("explicit preservation decision applies migration");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(home.join(".cc-switch/skills/review").is_dir());
    assert_eq!(
        fs::read_to_string(hermes_file).expect("read preserved Hermes state"),
        "external Hermes state"
    );
    assert!(completed.items.iter().any(|item| {
        item.action == SkillsMigrationAction::PreserveUnsupportedConsumerFiles
            && item.outcome == SkillsMigrationItemOutcome::Preserved
    }));
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());

    let service = SkillsMigrationExecutionService::new(state.db.clone());
    let report = service
        .inspect_latest_report()
        .expect("inspect completed migration report")
        .expect("completed migration report");
    assert_eq!(
        report.state,
        cc_switch_lib::SkillsMigrationReportState::Completed
    );
    assert_eq!(report.summary.preserved, 1);
    assert_eq!(report.summary.open, 0);
    assert!(report.findings.iter().any(|finding| {
        finding.disposition == cc_switch_lib::SkillsMigrationDisposition::PreserveWithConsent
            && finding.action == Some(SkillsMigrationAction::PreserveUnsupportedConsumerFiles)
            && finding.unsupported_consumers == vec!["gemini".to_string()]
    }));
    let finding_id = report.findings[0].finding_id.clone();
    assert_eq!(
        service
            .reveal_finding(SkillsMigrationFindingRevealIntent {
                finding_id: finding_id.clone(),
                observation_token: Some(report.observation_token.clone()),
            })
            .expect("reveal persisted finding by opaque identity"),
        source.parent().expect("source parent")
    );
    assert!(service
        .reveal_finding(SkillsMigrationFindingRevealIntent {
            finding_id: source.to_string_lossy().into_owned(),
            observation_token: None,
        })
        .is_err());
    assert!(service
        .reveal_finding(SkillsMigrationFindingRevealIntent {
            finding_id: "finding:missing-run:unknown".to_string(),
            observation_token: None,
        })
        .is_err());
    assert!(service
        .reveal_finding(SkillsMigrationFindingRevealIntent {
            finding_id,
            observation_token: Some("stale-report-observation".to_string()),
        })
        .is_err());
    assert_eq!(
        SkillsMigrationPreviewService::new(state.db.clone())
            .inspect()
            .expect("inspect after report query")
            .page_mode,
        cc_switch_lib::SkillsMigrationPageMode::Writable
    );
}

#[test]
fn completed_legacy_preserve_journal_is_backfilled_idempotently() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let mut legacy = installed_skill("review", false, false);
    legacy.apps.hermes = true;
    state.db.save_skill(&legacy).expect("seed legacy Skill row");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: true,
        })
        .expect("complete migration with consent");
    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    let service = SkillsMigrationExecutionService::new(state.db.clone());
    let run_id = completed
        .backup
        .as_ref()
        .expect("verified run backup")
        .backup_id
        .clone();
    let before = service
        .inspect_latest_report()
        .expect("inspect report before legacy rewrite")
        .expect("report before legacy rewrite");
    assert_eq!(before.summary.preserved, 1);
    assert_eq!(before.findings.len(), 1);

    let database_path = home.join(".cc-switch/cc-switch.db");
    drop(service);
    drop(state);
    let connection = Connection::open(&database_path).expect("open migration database");
    connection
        .execute(
            "DELETE FROM skills_migration_findings WHERE run_id = ?1",
            [&run_id],
        )
        .expect("remove report findings to emulate an older journal");
    connection
        .execute(
            "UPDATE skills_migration_runs
             SET accepted_preflight_snapshot_version = NULL,
                 accepted_preflight_snapshot = NULL
             WHERE id = ?1",
            [&run_id],
        )
        .expect("remove report snapshot to emulate an older journal");
    drop(connection);

    let state = create_test_state().expect("reopen migration state");
    let service = SkillsMigrationExecutionService::new(state.db.clone());
    let after = service
        .inspect_latest_report()
        .expect("backfill legacy migration report")
        .expect("backfilled report");
    assert_eq!(after.summary, before.summary);
    let mut expected_findings = before.findings.clone();
    expected_findings[0].origin = "legacy_backfill".to_string();
    assert_eq!(after.findings, expected_findings);
    assert_eq!(
        after.findings[0].unsupported_consumers,
        vec!["hermes".to_string()]
    );
    assert!(after.findings[0].detail_complete);

    let repeated = service
        .inspect_latest_report()
        .expect("repeat backfill report query")
        .expect("repeat backfilled report");
    assert_eq!(repeated.findings, after.findings);
    assert_eq!(repeated.summary, after.summary);
}

#[test]
fn incomplete_legacy_evidence_stays_visible_and_repairs_when_verified_backup_returns() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let mut legacy = installed_skill("review", false, false);
    legacy.apps.hermes = true;
    state.db.save_skill(&legacy).expect("seed legacy Skill row");
    let source = home.join(".cc-switch/skills/review");
    write_skill(&source, "review");
    let hermes_file = home.join(".hermes/skills/review/keep.txt");
    fs::create_dir_all(hermes_file.parent().unwrap()).expect("create Hermes Skill directory");
    fs::write(&hermes_file, "user-owned Hermes state").expect("write Hermes state");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: true,
        })
        .expect("complete migration with consent");
    let run_id = completed
        .backup
        .expect("verified migration backup")
        .backup_id;
    let backup_root = home
        .join(".cc-switch/skills-migration-backups")
        .join(&run_id);
    let marker_path = backup_root.join("backup-verified");
    let marker_bytes = fs::read(&marker_path).expect("read verified marker");
    let backup_database_path = backup_root.join("database.db");
    let backup_database_bytes =
        fs::read(&backup_database_path).expect("read migration database backup");
    let source_bytes = fs::read(source.join("SKILL.md")).expect("read Library source");
    let hermes_bytes = fs::read(&hermes_file).expect("read Hermes state");

    let database_path = home.join(".cc-switch/cc-switch.db");
    drop(state);
    let connection = Connection::open(&database_path).expect("open migration database");
    connection
        .execute(
            "DELETE FROM skills_migration_findings WHERE run_id = ?1",
            [&run_id],
        )
        .expect("remove findings to emulate a legacy journal");
    connection
        .execute(
            "UPDATE skills_migration_runs
             SET accepted_preflight_snapshot_version = NULL,
                 accepted_preflight_snapshot = NULL
             WHERE id = ?1",
            [&run_id],
        )
        .expect("remove snapshot to emulate a legacy journal");
    drop(connection);
    fs::remove_file(&marker_path).expect("make backup evidence temporarily unavailable");

    let state = create_test_state().expect("reopen migration state");
    let service = SkillsMigrationExecutionService::new(state.db.clone());
    let incomplete = service
        .inspect_latest_report()
        .expect("inspect incomplete legacy report")
        .expect("incomplete legacy report");
    assert_eq!(incomplete.summary.preserved, 1);
    assert_eq!(incomplete.summary.open, 1);
    assert_eq!(incomplete.findings.len(), 1);
    assert_eq!(incomplete.findings[0].status, "incomplete");
    assert_eq!(incomplete.findings[0].origin, "legacy_backfill");
    assert!(!incomplete.findings[0].detail_complete);
    assert!(incomplete.findings[0].unsupported_consumers.is_empty());
    assert!(
        !incomplete
            .backup
            .as_ref()
            .expect("backup reference")
            .restore_available
    );
    assert_eq!(
        service
            .inspect_latest_report()
            .expect("repeat incomplete report query")
            .expect("repeated incomplete report"),
        incomplete
    );
    assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), source_bytes);
    assert_eq!(fs::read(&hermes_file).unwrap(), hermes_bytes);
    assert_eq!(
        fs::read(&backup_database_path).unwrap(),
        backup_database_bytes
    );
    assert!(!marker_path.exists());

    fs::write(&marker_path, marker_bytes).expect("restore verified marker");
    let repaired = service
        .inspect_latest_report()
        .expect("repair legacy evidence")
        .expect("repaired legacy report");
    assert_eq!(repaired.summary.preserved, 1);
    assert_eq!(repaired.summary.open, 0);
    assert_eq!(repaired.findings[0].status, "preserved");
    assert_eq!(repaired.findings[0].origin, "legacy_backfill");
    assert!(repaired.findings[0].detail_complete);
    assert_eq!(
        repaired.findings[0].unsupported_consumers,
        vec!["hermes".to_string()]
    );
    assert!(
        repaired
            .backup
            .as_ref()
            .expect("repaired backup")
            .restore_available
    );
    assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), source_bytes);
    assert_eq!(fs::read(&hermes_file).unwrap(), hermes_bytes);
    assert_eq!(
        fs::read(&backup_database_path).unwrap(),
        backup_database_bytes
    );
}

#[test]
fn restore_retains_acknowledged_report_and_preserves_unsupported_consumer_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let mut legacy = installed_skill("review", false, false);
    legacy.apps.hermes = true;
    state.db.save_skill(&legacy).expect("seed legacy Skill row");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    let hermes_file = home.join(".hermes/skills/review/keep.txt");
    fs::create_dir_all(hermes_file.parent().unwrap()).expect("create Hermes Skill directory");
    fs::write(&hermes_file, "preserve through restore").expect("write Hermes state");
    let hermes_before = fs::read(&hermes_file).expect("read Hermes state");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: true,
        })
        .expect("complete migration");
    let backup_id = completed
        .backup
        .expect("verified migration backup")
        .backup_id;
    let service = SkillsMigrationExecutionService::new(state.db.clone());
    let report = service
        .inspect_latest_report()
        .expect("inspect completed report")
        .expect("completed report");
    let acknowledged = service
        .acknowledge_report(SkillsMigrationReportAckIntent {
            run_id: report.run_id.clone(),
        })
        .expect("acknowledge report");
    let acknowledged_at = acknowledged
        .acknowledged_at
        .expect("acknowledgement timestamp");
    let findings_before = acknowledged.findings.clone();

    let restored = service
        .restore(SkillsMigrationRestoreIntent { backup_id })
        .expect("restore verified backup");
    assert_eq!(restored.outcome, SkillsMigrationExecutionOutcome::Restored);
    let restored_report = service
        .inspect_latest_report()
        .expect("inspect restored report")
        .expect("restored report");
    assert_eq!(
        restored_report.state,
        cc_switch_lib::SkillsMigrationReportState::Restored
    );
    assert_eq!(restored_report.acknowledged_at, Some(acknowledged_at));
    assert_eq!(restored_report.findings, findings_before);
    assert_eq!(fs::read(&hermes_file).unwrap(), hermes_before);
}

#[test]
fn identical_dual_sources_are_both_retired_after_migration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[
            {"directory":"shared","app_type":"claude","installed":true},
            {"directory":"shared","app_type":"codex","installed":true}
        ]"#,
    );
    write_skill(&home.join(".claude/skills/shared"), "shared");
    write_skill(&home.join(".codex/skills/shared"), "shared");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect identical dual sources");

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("migrate identical dual sources");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(home.join(".claude/skills/shared").is_symlink());
    assert!(home.join(".agents/skills/shared").is_symlink());
    assert!(!home.join(".codex/skills/shared").exists());
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
}

#[test]
fn mixed_current_and_snapshot_sources_have_unique_durable_journal_items() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("shared", false, false))
        .expect("seed current legacy row");
    seed_snapshot(
        &state,
        r#"[
            {"directory":"shared","app_type":"claude","installed":true},
            {"directory":"shared","app_type":"codex","installed":true}
        ]"#,
    );
    write_skill(&home.join(".cc-switch/skills/shared"), "shared");
    write_skill(&home.join(".claude/skills/shared"), "shared");
    write_skill(&home.join(".codex/skills/shared"), "shared");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("journal accepts every distinct proven source");

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(state
        .db
        .get_library_skill_by_directory("shared")
        .unwrap()
        .is_some());
    assert!(home.join(".claude/skills/shared").is_symlink());
    assert!(home.join(".agents/skills/shared").is_symlink());
}

#[test]
fn backup_restore_recovers_exact_legacy_database_and_managed_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let legacy = installed_skill("review", true, false);
    state.db.save_skill(&legacy).expect("seed legacy Skill row");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/review"),
        home.join(".claude/skills/review"),
    )
    .unwrap();
    let original = fs::read(home.join(".cc-switch/skills/review/SKILL.md")).unwrap();
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    assert_eq!(
        preview
            .plan
            .iter()
            .find(|item| item.action == SkillsMigrationAction::CreateGlobalDeployment)
            .and_then(|item| item.from_location.as_deref()),
        Some(
            home.join(".claude/skills/review")
                .to_string_lossy()
                .as_ref()
        )
    );
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("complete migration");
    let reconciled = SkillDeploymentService::new(state.db.clone())
        .inspect(DeploymentQuery::for_target(DeploymentTarget::global(
            DeploymentConsumer::Claude,
        )))
        .expect("inspect reconciled deployment");
    assert!(
        reconciled.items.iter().any(|item| {
            item.target.consumer == DeploymentConsumer::Claude
                && item.status == DeploymentStatus::InSync
        }),
        "completed migration did not persist the desired deployment: {reconciled:#?}; execution: {completed:#?}"
    );
    let backup_id = completed.backup.unwrap().backup_id;
    fs::write(
        home.join(".cc-switch/skills/review/SKILL.md"),
        "post migration user change",
    )
    .unwrap();

    let wrong_backup = SkillsMigrationExecutionService::new(state.db.clone()).restore(
        SkillsMigrationRestoreIntent {
            backup_id: "another-run-backup".to_string(),
        },
    );
    assert!(wrong_backup.is_err());
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
    assert_eq!(state.db.get_all_installed_skills().unwrap().len(), 0);

    let restored = SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent { backup_id })
        .expect("restore protected migration backup");

    assert_eq!(restored.outcome, SkillsMigrationExecutionOutcome::Restored);
    assert_eq!(restored.page_mode, SkillsMigrationPageMode::ReadOnly);
    assert_eq!(
        fs::read(home.join(".cc-switch/skills/review/SKILL.md")).unwrap(),
        original
    );
    assert_eq!(state.db.get_all_installed_skills().unwrap().len(), 1);
    assert!(state.db.list_library_skills().unwrap().is_empty());
    let restored_link = home.join(".claude/skills/review");
    assert!(restored_link.is_symlink());
    assert_eq!(
        fs::read_link(restored_link).unwrap(),
        home.join(".cc-switch/skills/review")
    );
    assert!(restored
        .items
        .iter()
        .all(|item| item.outcome == cc_switch_lib::SkillsMigrationItemOutcome::RolledBack));
    let restore_activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            reason: Some(ActivityReason::CompensationRestore),
            ..ActivityQuery::default()
        })
        .unwrap();
    assert!(restore_activity.entries.iter().any(|entry| {
        entry.operation == ActivityOperation::Removal
            && entry.outcome == ActivityOutcome::RolledBack
    }));
}

#[test]
fn restore_missing_content_backup_requires_recovery_before_removing_outputs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("complete migration");
    let backup_id = completed.backup.unwrap().backup_id;
    let backup_content = home
        .join(".cc-switch/skills-migration-backups")
        .join(&backup_id)
        .join("content");
    let artifact = fs::read_dir(&backup_content)
        .unwrap()
        .next()
        .expect("content backup entry")
        .unwrap()
        .path();
    fs::remove_dir_all(artifact).unwrap();

    let recovery = SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent { backup_id })
        .expect("missing backup is a typed recovery outcome");

    assert_eq!(
        recovery.outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
    assert!(!recovery.backup.unwrap().restore_available);
    assert!(home.join(".cc-switch/skills/review").is_dir());
    assert!(home.join(".claude/skills/review").is_symlink());
    assert!(state
        .db
        .get_library_skill_by_directory("review")
        .unwrap()
        .is_some());
}

#[test]
fn restore_corrupt_database_backup_requires_recovery_before_removing_outputs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("complete migration");
    let backup_id = completed.backup.unwrap().backup_id;
    let database_backup = home
        .join(".cc-switch/skills-migration-backups")
        .join(&backup_id)
        .join("database.db");
    let mut bytes = fs::read(&database_backup).unwrap();
    bytes.push(0);
    fs::write(&database_backup, bytes).unwrap();

    let recovery = SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent { backup_id })
        .expect("corrupt backup is a typed recovery outcome");

    assert_eq!(
        recovery.outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
    assert!(!recovery.backup.unwrap().restore_available);
    assert!(home.join(".cc-switch/skills/review").is_dir());
    assert!(home.join(".claude/skills/review").is_symlink());
    assert!(state
        .db
        .get_library_skill_by_directory("review")
        .unwrap()
        .is_some());
}

#[test]
fn interrupted_restore_keeps_recovery_identity_after_database_replacement() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", true, false))
        .expect("seed legacy Skill row");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("complete migration");
    let backup_id = completed.backup.unwrap().backup_id;
    SkillsMigrationExecutionService::interrupt_after_database_restore_for_test(true);

    SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent {
            backup_id: backup_id.clone(),
        })
        .expect_err("simulate a crash after replacing the database");

    assert_eq!(state.db.get_all_installed_skills().unwrap().len(), 1);
    assert!(home.join(".cc-switch/skills/review").is_dir());
    let reloaded = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("reload recovery state");
    let execution = reloaded.execution.expect("recovery journal survives");
    assert_eq!(
        execution.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    let backup = execution.backup.expect("backup identity survives");
    assert_eq!(backup.backup_id, backup_id);
    assert!(backup.restore_available);
}

#[test]
fn restore_refuses_a_drifted_deployment_and_retains_recovery_journal_truth() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", true, false))
        .expect("seed legacy Skill row");
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/review"),
        home.join(".claude/skills/review"),
    )
    .unwrap();
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("complete migration");
    let backup_id = completed.backup.unwrap().backup_id;
    let deployment = home.join(".claude/skills/review");
    fs::remove_file(&deployment).unwrap();
    write_skill(&home.join("foreign/review"), "foreign-review");
    std::os::unix::fs::symlink(home.join("foreign/review"), &deployment).unwrap();

    let error = SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent { backup_id })
        .expect_err("restore must preserve a drifted deployment");

    assert!(error.to_string().contains("drifted deployment"));
    assert_eq!(
        fs::read_link(&deployment).unwrap(),
        home.join("foreign/review")
    );
    let reloaded = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("reload recovery journal");
    assert_eq!(
        reloaded.execution.unwrap().outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
}

#[test]
fn deployment_conflict_is_typed_and_preserves_foreign_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    fs::write(
        home.join(".claude/skills/review/foreign.txt"),
        "appeared late",
    )
    .unwrap();

    let result = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("late conflict is typed");
    assert_eq!(
        result.outcome,
        SkillsMigrationExecutionOutcome::StaleObservation
    );
    assert_eq!(
        fs::read_to_string(home.join(".claude/skills/review/foreign.txt")).unwrap(),
        "appeared late"
    );
}

#[test]
fn compensation_failure_requires_recovery_and_exposes_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect migration");
    SkillsMigrationExecutionService::fail_compensation_for_action_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));

    let result = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("compensation failure is typed");
    assert_eq!(
        result.outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
    assert!(result.backup.unwrap().restore_available);
    let reloaded = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("reload recovery journal");
    assert_eq!(
        reloaded.execution.unwrap().outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
}

#[test]
fn cleanup_retargeted_after_backup_is_blocked_and_preserved() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", false, true))
        .unwrap();
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/review"),
        home.join(".codex/skills/review"),
    )
    .unwrap();
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    assert_eq!(
        preview.status,
        cc_switch_lib::SkillsMigrationStatus::DecisionNeeded,
        "unexpected preview: {preview:#?}"
    );
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::RemoveLegacyCodexLink,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    let legacy = home.join(".codex/skills/review");
    fs::remove_file(&legacy).unwrap();
    write_skill(&home.join(".cc-switch/skills/other"), "other");
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/other"), &legacy).unwrap();

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("cleanup race is typed");

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert_eq!(
        fs::read_link(&legacy).unwrap(),
        home.join(".cc-switch/skills/other")
    );
    assert!(blocked.backup.unwrap().restore_available);
}

#[test]
fn same_hash_library_reuse_retires_real_legacy_source_before_deploying() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let seed = home.join("seed/review");
    write_skill(&seed, "review");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &seed,
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::LocalImport,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("review"),
    )
    .unwrap();
    write_skill(&home.join(".claude/skills/review"), "review");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    assert!(preview
        .plan
        .iter()
        .any(|item| item.action == SkillsMigrationAction::ReuseLibrary));

    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();

    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
    assert!(home.join(".claude/skills/review").is_symlink());
    assert_eq!(
        fs::read_link(home.join(".claude/skills/review")).unwrap(),
        home.join(".cc-switch/skills/review")
    );
}

#[test]
fn vanished_backup_source_blocks_before_any_item_execution() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    SkillsMigrationExecutionService::remove_source_before_backup_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert_eq!(blocked.progress.completed_items, 0);
    assert!(blocked.items.is_empty());
    assert!(state.db.list_library_skills().unwrap().is_empty());
    let after_failure = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("backup failure leaves no trapped active run");
    assert!(after_failure.execution.is_none());
    write_skill(&home.join(".claude/skills/review"), "review");
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: SkillsMigrationPreviewService::new(state.db.clone())
                .inspect()
                .unwrap()
                .observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .expect("retry after fixing backup source");
    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
}

#[test]
fn incomplete_verified_backup_is_discarded_before_resume_can_mutate() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let source = home.join(".claude/skills/review");
    write_skill(&source, "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::MoveToLibrary,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    let backup_id = interrupted.backup.unwrap().backup_id;
    fs::remove_file(
        home.join(".cc-switch/skills-migration-backups")
            .join(backup_id)
            .join("backup-verified"),
    )
    .unwrap();

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("incomplete pre-mutation backup is discarded");

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(blocked.backup.is_none());
    assert!(source.is_dir());
    assert!(state.db.list_library_skills().unwrap().is_empty());
    assert!(SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap()
        .execution
        .is_none());
}

#[test]
fn completed_item_drift_does_not_block_an_independent_pending_skill() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[
            {"directory":"alpha","app_type":"claude","installed":true},
            {"directory":"beta","app_type":"claude","installed":true}
        ]"#,
    );
    write_skill(&home.join(".claude/skills/alpha"), "alpha");
    write_skill(&home.join(".claude/skills/beta"), "beta");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::CreateGlobalDeployment,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable
    );
    fs::write(
        home.join(".cc-switch/skills/alpha/SKILL.md"),
        "---\nname: changed\ndescription: postcondition drift\n---\n",
    )
    .unwrap();

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("independent items continue after drift");

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(blocked.items.iter().any(|item| {
        item.directory.as_deref() == Some("alpha")
            && item.action == SkillsMigrationAction::MoveToLibrary
            && item.outcome == cc_switch_lib::SkillsMigrationItemOutcome::Blocked
    }));
    assert!(home.join(".cc-switch/skills/beta").is_dir());
    assert!(home.join(".claude/skills/beta").is_symlink());
}

#[test]
fn completed_migrations_retain_only_twenty_verified_backup_roots() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    for index in 0..21 {
        let directory = format!("review-{index:02}");
        seed_snapshot(
            &state,
            &format!(r#"[{{"directory":"{directory}","app_type":"claude","installed":true}}]"#),
        );
        write_skill(&home.join(".claude/skills").join(&directory), &directory);
        let preview = SkillsMigrationPreviewService::new(state.db.clone())
            .inspect()
            .unwrap();
        let result = SkillsMigrationExecutionService::new(state.db.clone())
            .start(SkillsMigrationIntent {
                observation_token: preview.observation_token,
                preserve_unsupported_consumer_files: false,
            })
            .unwrap();
        assert_eq!(result.outcome, SkillsMigrationExecutionOutcome::Completed);
    }

    let roots = fs::read_dir(home.join(".cc-switch/skills-migration-backups"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 20);
    assert!(roots
        .iter()
        .all(|entry| entry.path().join("backup-verified").is_file()));
}

#[test]
fn resume_revalidates_completed_outputs_before_finalization() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    write_skill(&home.join(".claude/skills/review"), "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    SkillsMigrationExecutionService::interrupt_before_action_for_test(Some(
        SkillsMigrationAction::Finalize,
    ));
    let interrupted = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    assert_eq!(
        interrupted.outcome,
        SkillsMigrationExecutionOutcome::Resumable,
        "expected finalization interruption: {interrupted:#?}"
    );
    let official = home.join(".claude/skills/review");
    let official_metadata = fs::symlink_metadata(&official).unwrap();
    if official_metadata.file_type().is_symlink() || official_metadata.is_file() {
        fs::remove_file(&official).unwrap();
    } else {
        fs::remove_dir_all(&official).unwrap();
    }

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .unwrap();

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(state
        .db
        .get_setting("skills_ssot_migration_snapshot")
        .unwrap()
        .is_some());
    assert!(blocked.backup.unwrap().restore_available);
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .resume()
        .expect("retry repairs a resolvable missing deployment");
    assert_eq!(
        completed.outcome,
        SkillsMigrationExecutionOutcome::Completed
    );
}

#[test]
fn restore_preserves_occupied_legacy_cleanup_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("review", false, true))
        .unwrap();
    write_skill(&home.join(".cc-switch/skills/review"), "review");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/review"),
        home.join(".codex/skills/review"),
    )
    .unwrap();
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    let legacy = home.join(".codex/skills/review");
    write_skill(&legacy, "foreign");

    SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent {
            backup_id: completed.backup.unwrap().backup_id,
        })
        .expect_err("occupied legacy path must block restore");

    assert!(fs::read_to_string(legacy.join("SKILL.md"))
        .unwrap()
        .contains("foreign"));
    assert_eq!(
        SkillsMigrationPreviewService::new(state.db.clone())
            .inspect()
            .unwrap()
            .execution
            .unwrap()
            .outcome,
        SkillsMigrationExecutionOutcome::RecoveryRequired
    );
}

#[test]
fn restore_preserves_occupied_retired_source_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let custom = home.join("custom-claude");
    let settings = cc_switch_lib::AppSettings {
        claude_config_dir: Some(custom.to_string_lossy().into_owned()),
        ..Default::default()
    };
    cc_switch_lib::update_settings(settings).unwrap();
    let state = create_test_state().expect("create test state");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let source = custom.join("skills/review");
    write_skill(&source, "review");
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    let completed = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();
    write_skill(&source, "foreign");

    SkillsMigrationExecutionService::new(state.db.clone())
        .restore(SkillsMigrationRestoreIntent {
            backup_id: completed.backup.unwrap().backup_id,
        })
        .expect_err("occupied retired source must block restore");

    assert!(fs::read_to_string(source.join("SKILL.md"))
        .unwrap()
        .contains("foreign"));
}

#[test]
fn unresolved_managed_conflict_never_creates_or_finalizes_a_run() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let seed = home.join("seed/review");
    write_skill(&seed, "library");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &seed,
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::LocalImport,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("review"),
    )
    .unwrap();
    write_skill(&home.join(".claude/skills/review"), "legacy-different");
    seed_snapshot(
        &state,
        r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
    );
    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .unwrap();
    assert_eq!(
        preview.status,
        cc_switch_lib::SkillsMigrationStatus::Blocked
    );

    let blocked = SkillsMigrationExecutionService::new(state.db.clone())
        .start(SkillsMigrationIntent {
            observation_token: preview.observation_token,
            preserve_unsupported_consumer_files: false,
        })
        .unwrap();

    assert_eq!(blocked.outcome, SkillsMigrationExecutionOutcome::Blocked);
    assert!(blocked.backup.is_none());
    assert_eq!(
        state
            .db
            .get_setting("skills_ssot_migration_snapshot")
            .unwrap()
            .as_deref(),
        Some(r#"[{"directory":"review","app_type":"claude","installed":true}]"#)
    );
    assert!(home.join(".claude/skills/review").is_dir());
}
