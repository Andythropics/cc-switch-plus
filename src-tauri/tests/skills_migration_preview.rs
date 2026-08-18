#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::symlink;

use cc_switch_lib::{
    ConsumerCompatibility, InstalledSkill, LibrarySkill, LibrarySkillCompatibility,
    LibrarySkillSource, LibrarySourceKind, SkillApps, SkillsMigrationAction,
    SkillsMigrationInventoryKind, SkillsMigrationInventoryState, SkillsMigrationPageMode,
    SkillsMigrationPreviewService, SkillsMigrationReason, SkillsMigrationRevealIntent,
    SkillsMigrationStatus,
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

fn library_skill(directory: &str, content_hash: String) -> LibrarySkill {
    LibrarySkill {
        id: format!("library:{directory}"),
        directory: directory.to_string(),
        display_name: directory.to_string(),
        description: None,
        source: LibrarySkillSource {
            kind: LibrarySourceKind::LocalImport,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        compatibility: LibrarySkillCompatibility {
            claude: ConsumerCompatibility {
                compatible: true,
                issues: vec![],
            },
            codex: ConsumerCompatibility {
                compatible: true,
                issues: vec![],
            },
        },
        content_hash,
        acquired_at: 1,
        updated_at: 1,
    }
}

#[test]
fn root_system_metadata_is_ignored_instead_of_blocking_migration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending decision");
    let roots = [
        home.join(".cc-switch/skills"),
        home.join(".codex/skills"),
        home.join(".agents/skills"),
    ];
    for root in &roots {
        fs::create_dir_all(root).expect("create scanned root");
        for name in [".DS_Store", ".localized", "._skill"] {
            fs::write(root.join(name), b"finder metadata").expect("write metadata");
        }
    }

    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect metadata-only roots");

    assert_eq!(preview.status, SkillsMigrationStatus::DecisionNeeded);
    assert!(preview.backup.ready);
    for name in [".DS_Store", ".localized", "._skill"] {
        assert!(preview
            .inventory
            .iter()
            .all(|item| item.directory.as_deref() != Some(name)));
        assert!(preview
            .plan
            .iter()
            .all(|item| item.directory.as_deref() != Some(name)));
    }
    for root in &roots {
        for name in [".DS_Store", ".localized", "._skill"] {
            fs::write(root.join(name), b"updated finder metadata").expect("update metadata");
        }
    }
    assert_eq!(
        SkillsMigrationPreviewService::new(state.db.clone())
            .inspect()
            .expect("reinspect metadata-only roots")
            .observation_token,
        preview.observation_token
    );
}

#[test]
fn unsupported_legacy_consumers_offer_an_explicit_preserve_resolution() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    let mut legacy = installed_skill("review", true, true);
    legacy.apps.hermes = true;
    state.db.save_skill(&legacy).expect("seed legacy Skill");
    write_skill(&home.join(".cc-switch/skills/review"), "review");

    let preview = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect unsupported Consumer state");
    let resolution = preview
        .plan
        .iter()
        .find(|item| {
            item.directory.as_deref() == Some("review")
                && item.reason == SkillsMigrationReason::UnsupportedConsumerEnabled
        })
        .expect("unsupported Consumer resolution");

    assert_eq!(preview.status, SkillsMigrationStatus::DecisionNeeded);
    assert!(preview.backup.ready);
    assert_eq!(
        resolution.action,
        SkillsMigrationAction::PreserveUnsupportedConsumerFiles
    );
    assert_eq!(resolution.unsupported_consumers, vec!["hermes"]);
}

#[test]
fn reveal_resolution_is_bound_to_the_current_observation_and_plan_index() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending decision");
    let root = home.join(".codex/skills");
    fs::create_dir_all(&root).expect("create legacy Codex root");
    let conflict = root.join("foreign");
    fs::write(&conflict, "occupied").expect("write foreign conflict");
    let service = SkillsMigrationPreviewService::new(state.db.clone());
    let preview = service.inspect().expect("inspect conflict");
    let plan_index = preview
        .plan
        .iter()
        .position(|item| item.directory.as_deref() == Some("foreign"))
        .expect("find conflict plan item");

    let revealed = service
        .resolve_reveal_directory(SkillsMigrationRevealIntent {
            observation_token: preview.observation_token.clone(),
            plan_index,
        })
        .expect("resolve trusted containing directory");
    assert_eq!(revealed, root);

    fs::write(&conflict, "changed").expect("change observed conflict");
    assert!(service
        .resolve_reveal_directory(SkillsMigrationRevealIntent {
            observation_token: preview.observation_token,
            plan_index,
        })
        .is_err());
}

#[test]
fn preflight_is_stable_and_byte_for_byte_read_only_for_legacy_decision_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending decision");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        )
        .expect("seed legacy snapshot");
    write_skill(&home.join(".claude/skills/review"), "review");

    let database_path = home.join(".cc-switch/cc-switch.db");
    let database_before = fs::read(&database_path).expect("read database before preflight");
    let skill_before =
        fs::read(home.join(".claude/skills/review/SKILL.md")).expect("read Skill before preflight");

    let service = SkillsMigrationPreviewService::new(state.db.clone());
    let first = service.inspect().expect("inspect first preflight");
    let second = service.inspect().expect("inspect stable preflight");

    assert_eq!(first.status, SkillsMigrationStatus::DecisionNeeded);
    assert_eq!(first, second);
    assert_eq!(
        fs::read(&database_path).expect("read database after preflight"),
        database_before
    );
    assert_eq!(
        fs::read(home.join(".claude/skills/review/SKILL.md")).expect("read Skill after preflight"),
        skill_before
    );
    assert!(!home.join(".agents").exists());
    assert!(!home.join(".cc-switch/skills").exists());
    assert!(!home.join(".cc-switch/skill-backups").exists());
}

#[test]
fn library_only_state_does_not_offer_legacy_migration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source = home.join("source/library-only");
    write_skill(&source, "library-only");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        LibrarySkillSource {
            kind: LibrarySourceKind::LocalImport,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("library-only"),
    )
    .expect("seed redesigned Library only");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect");
    assert_eq!(result.status, SkillsMigrationStatus::NotRequired);
    assert_eq!(result.page_mode, SkillsMigrationPageMode::Writable);
}

#[test]
fn legacy_installed_skill_without_pending_still_requires_decision() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    write_skill(
        &home.join(".cc-switch/skills/current-legacy"),
        "current-legacy",
    );
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("current-legacy", true, false))
        .expect("save legacy row");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect");
    assert_eq!(result.status, SkillsMigrationStatus::DecisionNeeded);
    assert_eq!(result.page_mode, SkillsMigrationPageMode::ReadOnly);
    assert!(result
        .plan
        .iter()
        .any(|item| item.action == SkillsMigrationAction::ReuseLibrary));
}

#[test]
fn malformed_snapshot_and_invalid_skill_fail_closed() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    fs::create_dir_all(home.join(".claude/skills/not-a-skill")).expect("create invalid source");
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending");
    state
        .db
        .set_setting("skills_ssot_migration_snapshot", "not-json")
        .expect("seed malformed snapshot");
    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect malformed");
    assert_eq!(result.status, SkillsMigrationStatus::Blocked);
    assert!(!result.backup.ready);
    assert!(result.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::ScanError
            && item.state == SkillsMigrationInventoryState::Invalid
    }));

    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"not-a-skill","app_type":"claude","installed":true}]"#,
        )
        .expect("seed valid evidence");
    let invalid = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect invalid Skill");
    assert_eq!(invalid.status, SkillsMigrationStatus::Blocked);
    assert!(invalid.plan.iter().any(|item| {
        item.reason == SkillsMigrationReason::InvalidLegacyState
            && item.action == SkillsMigrationAction::ResolveConflict
    }));
}

#[test]
fn library_identity_is_verified_and_content_drift_changes_token() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_path = home.join("source/review");
    write_skill(&source_path, "review");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_path,
        library_skill("review", String::new()).source,
        Some("review"),
    )
    .expect("seed valid Library");
    let library_path = home.join(".cc-switch/skills/review");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending");

    let service = SkillsMigrationPreviewService::new(state.db.clone());
    let first = service.inspect().expect("inspect exact Library");
    fs::write(library_path.join("SKILL.md"), "changed bytes").expect("drift Library bytes");
    let drifted = service.inspect().expect("inspect drifted Library");
    assert_ne!(first.observation_token, drifted.observation_token);
    assert_eq!(drifted.status, SkillsMigrationStatus::Blocked);
    assert!(drifted.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::ManagedLibrary
            && item.state == SkillsMigrationInventoryState::Invalid
    }));
}

#[test]
fn differing_legacy_and_library_content_requires_user_resolution() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let library_source = home.join("source/review");
    let legacy_path = home.join(".claude/skills/review");
    write_skill(&library_source, "library-review");
    write_skill(&legacy_path, "legacy-review");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &library_source,
        library_skill("review", String::new()).source,
        Some("review"),
    )
    .expect("seed valid Library");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        )
        .expect("seed snapshot");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect conflict");
    assert!(result.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::ResolveConflict
            && item.reason == SkillsMigrationReason::ContentConflict
    }));
    assert!(result
        .inventory
        .iter()
        .any(|item| item.kind == SkillsMigrationInventoryKind::TargetConflict));
}

#[test]
fn byte_identical_directory_replacement_changes_observation_token() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = home.join(".claude/skills/review");
    write_skill(&path, "review");
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        )
        .expect("seed snapshot");
    let service = SkillsMigrationPreviewService::new(state.db.clone());
    let before = service.inspect().expect("inspect before replacement");
    let bytes = fs::read(path.join("SKILL.md")).expect("read fixture bytes");
    fs::remove_dir_all(&path).expect("replace directory identity");
    fs::create_dir_all(&path).expect("recreate directory");
    fs::write(path.join("SKILL.md"), bytes).expect("restore identical bytes");
    let after = service.inspect().expect("inspect after replacement");
    assert_ne!(before.observation_token, after.observation_token);
}

#[test]
fn identical_library_content_is_reused_without_a_second_move() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source = home.join("source/review");
    let legacy = home.join(".claude/skills/review");
    write_skill(&source, "review");
    write_skill(&legacy, "review");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        library_skill("review", String::new()).source,
        Some("review"),
    )
    .expect("seed Library");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"review","app_type":"claude","installed":true}]"#,
        )
        .expect("seed snapshot");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect reuse");
    assert!(result
        .plan
        .iter()
        .any(|item| item.action == SkillsMigrationAction::ReuseLibrary));
    assert!(!result
        .plan
        .iter()
        .any(|item| item.action == SkillsMigrationAction::MoveToLibrary));
}

#[test]
fn codex_legacy_and_unmanaged_entries_are_exhaustively_classified() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source = home.join("source/managed");
    write_skill(&source, "managed");
    let state = create_test_state().expect("create test state");
    cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        library_skill("managed", String::new()).source,
        Some("managed"),
    )
    .expect("seed Library");
    state
        .db
        .set_setting("skills_ssot_migration_pending", "true")
        .expect("seed pending");
    let codex = home.join(".codex/skills");
    fs::create_dir_all(&codex).expect("create old Codex root");
    symlink(
        home.join(".cc-switch/skills/managed"),
        codex.join("managed"),
    )
    .expect("create proven managed link");
    write_skill(&home.join("foreign"), "foreign");
    symlink(home.join("foreign"), codex.join("foreign")).expect("create foreign link");
    symlink(home.join("missing"), codex.join("broken")).expect("create broken link");
    fs::write(codex.join("occupied"), "not a Skill directory").expect("create occupied file");
    write_skill(&home.join(".claude/skills/unmanaged"), "unmanaged");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect legacy entries");
    for (directory, state) in [
        ("managed", SkillsMigrationInventoryState::ManagedLink),
        ("foreign", SkillsMigrationInventoryState::ForeignLink),
        ("broken", SkillsMigrationInventoryState::BrokenLink),
        ("occupied", SkillsMigrationInventoryState::Occupied),
    ] {
        assert!(
            result.inventory.iter().any(|item| {
                item.kind == SkillsMigrationInventoryKind::LegacyCodexEntry
                    && item.directory.as_deref() == Some(directory)
                    && item.state == state
            }),
            "missing Codex classification for {directory}"
        );
    }
    assert!(result.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::UnmanagedContent
            && item.directory.as_deref() == Some("unmanaged")
            && item.state == SkillsMigrationInventoryState::RealDirectory
    }));
    assert!(result.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::RemoveLegacyCodexLink
            && item.directory.as_deref() == Some("managed")
    }));
    assert!(result.backup.ready);
}

#[test]
fn codex_link_to_unmanaged_alternate_root_is_preserved_for_cc_switch_storage() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let mut settings = cc_switch_lib::AppSettings::default();
    settings.skill_storage_location = cc_switch_lib::SkillStorageLocation::CcSwitch;
    cc_switch_lib::update_settings(settings).expect("select CcSwitch legacy SSOT");
    write_skill(&home.join(".cc-switch/skills/foo"), "foo");
    write_skill(&home.join(".agents/skills/foo"), "foo");
    fs::create_dir_all(home.join(".codex/skills")).expect("create old Codex root");
    symlink(
        home.join(".agents/skills/foo"),
        home.join(".codex/skills/foo"),
    )
    .expect("link old Codex entry to unmanaged alternate root");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("foo", true, true))
        .expect("save legacy row");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect legacy entry");
    assert!(result.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::LegacyCodexEntry
            && item.directory.as_deref() == Some("foo")
            && item.state == SkillsMigrationInventoryState::ForeignLink
    }));
    assert!(result.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::ResolveConflict
            && item.disposition == cc_switch_lib::SkillsMigrationDisposition::UserResolve
            && item.directory.as_deref() == Some("foo")
            && item.consumer == Some(cc_switch_lib::DeploymentConsumer::Codex)
            && item.reason == SkillsMigrationReason::ForeignOrAmbiguous
    }));
    assert!(!result.plan.iter().any(|item| {
        item.action == SkillsMigrationAction::RemoveLegacyCodexLink
            && item.directory.as_deref() == Some("foo")
    }));
}

#[test]
fn configured_legacy_root_is_observed_without_mutation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let override_root = home.join("custom-claude");
    write_skill(
        &override_root.join("skills/override-skill"),
        "override-skill",
    );
    let mut settings = cc_switch_lib::AppSettings::default();
    settings.claude_config_dir = Some(override_root.to_string_lossy().into_owned());
    cc_switch_lib::update_settings(settings).expect("set Claude override");
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"override-skill","app_type":"claude","installed":true}]"#,
        )
        .expect("seed snapshot");
    let bytes = fs::read(override_root.join("skills/override-skill/SKILL.md"))
        .expect("read before inspect");

    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect override");
    assert!(result.inventory.iter().any(|item| {
        item.directory.as_deref() == Some("override-skill")
            && item
                .location
                .contains("custom-claude/skills/override-skill")
    }));
    assert_eq!(
        fs::read(override_root.join("skills/override-skill/SKILL.md")).expect("read after inspect"),
        bytes
    );
}

#[test]
fn missing_enabled_source_blocks_backup_prerequisites() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"missing","app_type":"claude","installed":true}]"#,
        )
        .expect("seed missing source");
    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect missing source");
    assert_eq!(result.status, SkillsMigrationStatus::Blocked);
    assert!(!result.backup.ready);
    assert!(result
        .plan
        .iter()
        .any(|item| item.reason == SkillsMigrationReason::MissingSource));
}

#[test]
fn current_legacy_ssot_honors_configured_storage_and_detects_other_root_conflict() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let mut settings = cc_switch_lib::AppSettings::default();
    settings.skill_storage_location = cc_switch_lib::SkillStorageLocation::Unified;
    cc_switch_lib::update_settings(settings).expect("select unified legacy SSOT");
    write_skill(&home.join(".agents/skills/current"), "unified-current");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&installed_skill("current", true, true))
        .expect("save current legacy row");

    let initial = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect unified source");
    assert!(initial.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::LegacySkill
            && item.location.contains(".agents/skills/current")
            && item.state == SkillsMigrationInventoryState::RealDirectory
    }));
    assert_eq!(
        initial
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::MoveToLibrary)
            .count(),
        1,
        "two enabled consumers share one legacy SSOT move"
    );

    write_skill(&home.join(".cc-switch/skills/current"), "different-current");
    let conflict = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect conflicting SSOT roots");
    assert!(conflict.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::TargetConflict
            && item.directory.as_deref() == Some("current")
    }));
    assert!(conflict
        .plan
        .iter()
        .any(|item| item.reason == SkillsMigrationReason::ContentConflict));
}

#[test]
fn valid_internal_symlink_keeps_backup_prerequisites_ready() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source = home.join(".claude/skills/linked-assets");
    write_skill(&source, "linked-assets");
    fs::create_dir_all(source.join("assets")).expect("create assets");
    fs::write(source.join("assets/checklist.txt"), "check").expect("write asset");
    symlink("assets/checklist.txt", source.join("checklist.txt")).expect("internal link");
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"linked-assets","app_type":"claude","installed":true}]"#,
        )
        .expect("seed snapshot");
    let result = SkillsMigrationPreviewService::new(state.db.clone())
        .inspect()
        .expect("inspect internal link");
    assert_eq!(result.status, SkillsMigrationStatus::DecisionNeeded);
    assert!(result.backup.ready);
}

#[test]
fn dual_consumer_sources_coalesce_when_identical_and_conflict_when_different() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let claude = home.join(".claude/skills/shared");
    let codex = home.join(".codex/skills/shared");
    write_skill(&claude, "shared");
    write_skill(&codex, "shared");
    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[
                {"directory":"shared","app_type":"claude","installed":true},
                {"directory":"shared","app_type":"codex","installed":true}
            ]"#,
        )
        .expect("seed dual snapshot");
    let service = SkillsMigrationPreviewService::new(state.db.clone());
    let identical = service.inspect().expect("inspect identical sources");
    assert_eq!(
        identical
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::MoveToLibrary)
            .count(),
        1
    );
    assert_eq!(
        identical
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::ReuseLibrary)
            .count(),
        1
    );
    assert_eq!(
        identical
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::CreateGlobalDeployment)
            .count(),
        2
    );

    write_skill(&codex, "different-shared");
    let conflict = service.inspect().expect("inspect differing sources");
    assert_eq!(
        conflict
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::MoveToLibrary)
            .count(),
        0
    );
    assert_eq!(
        conflict
            .plan
            .iter()
            .filter(|item| item.action == SkillsMigrationAction::CreateGlobalDeployment)
            .count(),
        0
    );
    assert!(conflict.inventory.iter().any(|item| {
        item.kind == SkillsMigrationInventoryKind::TargetConflict
            && item.directory.as_deref() == Some("shared")
    }));
}
