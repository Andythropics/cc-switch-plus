#![cfg(target_os = "macos")]

use std::fs;
use std::path::Path;

use cc_switch_lib::{
    ActivityOperation, ActivityOutcome, ActivityQuery, ActivityReason, ActivityRecorder,
    DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome,
    DeploymentTarget, LibrarySkillAcquisitionService, LibrarySkillSource,
    LibrarySkillUpdateApplyIntent, LibrarySkillUpdateApplyOutcome, LibrarySkillUpdateCheckOutcome,
    LibrarySkillUpdateReason, LibrarySkillUpdateService, LibrarySourceKind,
    ProjectWorkspaceService, SkillDeploymentService, WorkspaceKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(root: &std::path::Path, body: &str) {
    fs::create_dir_all(root).expect("create Skill root");
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: update-check\ndescription: Update check fixture\n---\n\n{body}\n"),
    )
    .expect("write SKILL.md");
}

fn git_source() -> LibrarySkillSource {
    LibrarySkillSource {
        kind: LibrarySourceKind::Git,
        url: Some("https://github.com/example/skills/blob/main/update-check/SKILL.md".into()),
        repo_owner: Some("example".into()),
        repo_name: Some("skills".into()),
        repo_branch: Some("main".into()),
        skill_path: Some("update-check".into()),
        marketplace: None,
    }
}

#[test]
fn update_inspection_reports_upstream_change_and_local_modification_without_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream");
    write_skill(&local, "local snapshot");
    write_skill(&upstream.join("update-check"), "upstream snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");

    let library_path = home.join(".cc-switch/skills/update-check/SKILL.md");
    fs::write(
        &library_path,
        "---\nname: update-check\ndescription: Update check fixture\n---\n\nlocal edit\n",
    )
    .expect("make local Library edit");
    let before = fs::read(&library_path).expect("snapshot Library bytes");

    let inspection = LibrarySkillUpdateService::inspect_from_repository_snapshot(
        &state.db, &skill.id, &upstream,
    )
    .expect("inspect upstream snapshot");

    assert_eq!(
        inspection.outcome,
        cc_switch_lib::LibrarySkillUpdateCheckOutcome::UpdateAvailable
    );
    assert!(inspection.local_modified);
    assert_ne!(inspection.live_content_hash, inspection.staged_content_hash);
    assert_eq!(
        fs::read(&library_path).expect("Library remains unchanged"),
        before
    );
}

#[test]
fn matching_live_and_upstream_still_stages_when_recorded_hash_is_stale() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");

    // Simulate an upstream update already copied into the live tree while the
    // recorded DB hash still describes the old snapshot.  This needs a stage
    // token so the user can explicitly confirm adoption and obtain a backup.
    fs::copy(
        upstream.join("SKILL.md"),
        home.join(".cc-switch/skills/update-check/SKILL.md"),
    )
    .expect("copy upstream bytes into live snapshot");
    let check = LibrarySkillUpdateService::inspect_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("inspect matching live/upstream snapshot");
    assert_eq!(
        check.outcome,
        LibrarySkillUpdateCheckOutcome::UpdateAvailable
    );
    assert!(check.local_modified);
    let staged = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage stale-recorded snapshot");
    assert!(staged.stage_token.is_some());
}

#[test]
fn invalid_upstream_stage_is_structured_and_leaves_live_snapshot_untouched() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    fs::create_dir_all(&upstream).expect("create invalid upstream directory");
    fs::write(upstream.join("README.md"), "missing canonical manifest")
        .expect("write invalid file");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let before =
        fs::read(home.join(".cc-switch/skills/update-check/SKILL.md")).expect("read live snapshot");

    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("invalid candidate should be structured");
    assert_eq!(
        check.outcome,
        LibrarySkillUpdateCheckOutcome::InvalidCandidate
    );
    assert!(check.stage_token.is_none());
    assert_eq!(
        fs::read(home.join(".cc-switch/skills/update-check/SKILL.md")).expect("live unchanged"),
        before
    );
    let stage_root = home.join(".cc-switch/skill-update-stages");
    assert!(
        !stage_root.exists()
            || fs::read_dir(stage_root)
                .expect("read stage root")
                .next()
                .is_none(),
        "invalid candidate must not leave a stage directory"
    );
}

#[test]
fn stale_update_token_does_not_mutate_live_snapshot_and_cleans_stage() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    let stage_token = check.stage_token.clone().expect("stage token");
    let before =
        fs::read(home.join(".cc-switch/skills/update-check/SKILL.md")).expect("read live snapshot");
    fs::write(
        home.join(".cc-switch/skills/update-check/SKILL.md"),
        "---\nname: update-check\ndescription: Update check fixture\n---\n\nconcurrent edit\n",
    )
    .expect("mutate live snapshot after staging");

    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id,
            stage_token,
            observation_token: check.observation_token,
            confirm_local_modifications: true,
        },
    )
    .expect("structured stale result");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Stale);
    assert_ne!(
        fs::read(home.join(".cc-switch/skills/update-check/SKILL.md")).expect("live remains"),
        before
    );
    let stage_root = home.join(".cc-switch/skill-update-stages");
    assert!(
        !stage_root.exists()
            || fs::read_dir(stage_root)
                .expect("read stage root")
                .next()
                .is_none(),
        "stale token must not leave staged content"
    );
}

#[test]
fn update_impacts_list_active_and_archived_targets_without_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let workspace_active_root = tempfile::tempdir().expect("create active workspace");
    let workspace_archived_root = tempfile::tempdir().expect("create archived workspace");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let workspaces = ProjectWorkspaceService::new(state.db.clone());
    let active = workspaces
        .register(workspace_active_root.path(), None)
        .expect("register active workspace")
        .workspace;
    let archived = workspaces
        .register(workspace_archived_root.path(), None)
        .expect("register archived workspace")
        .workspace;
    let deployment = SkillDeploymentService::new(state.db.clone());
    let active_target = cc_switch_lib::DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: active.id.clone(),
    };
    let archived_target = cc_switch_lib::DeploymentTarget {
        consumer: DeploymentConsumer::Codex,
        workspace: WorkspaceKind::Project,
        workspace_id: archived.id.clone(),
    };
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: active_target.clone(),
        }))
        .expect("deploy active target");
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: archived_target.clone(),
        }))
        .expect("deploy archived target");
    workspaces
        .archive(&archived.id)
        .expect("archive target workspace");
    let active_link = workspace_active_root
        .path()
        .join(".claude/skills/update-check");
    let archived_link = workspace_archived_root
        .path()
        .join(".agents/skills/update-check");
    let active_before = fs::read_link(&active_link).expect("read active link");
    let archived_before = fs::read_link(&archived_link).expect("read archived link");

    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    assert_eq!(check.affected_deployments.len(), 2);
    assert!(check
        .affected_deployments
        .iter()
        .any(|impact| impact.inspection.target.workspace_id == active.id));
    assert!(check
        .affected_deployments
        .iter()
        .any(|impact| impact.inspection.target.workspace_id == archived.id));

    assert!(check
        .affected_deployments
        .iter()
        .all(|impact| impact.staged_compatible));
    assert_eq!(
        fs::read_link(&active_link).expect("active link remains"),
        active_before
    );
    assert_eq!(
        fs::read_link(&archived_link).expect("archived link remains"),
        archived_before
    );
}

#[test]
fn database_update_failure_swaps_back_and_reports_rolled_back() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    state
        .db
        .fail_library_skill_updates_for_test()
        .expect("install update failpoint");
    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token: check.stage_token.expect("stage token"),
            observation_token: check.observation_token,
            confirm_local_modifications: false,
        },
    )
    .expect("structured DB failure result");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::RolledBack);
    assert!(result.backup_path.is_none());
    assert!(
        fs::read_to_string(home.join(".cc-switch/skills/update-check/SKILL.md"))
            .expect("read restored snapshot")
            .contains("old snapshot")
    );
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .expect("read Library row")
            .expect("Library row remains")
            .content_hash,
        skill.content_hash
    );
}

#[test]
fn injected_atomic_swap_failure_keeps_live_and_db_snapshot_unchanged() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    LibrarySkillUpdateService::force_atomic_swap_failure_for_test(true);
    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token: check.stage_token.expect("stage token"),
            observation_token: check.observation_token,
            confirm_local_modifications: false,
        },
    )
    .expect("structured swap failure result");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::RolledBack);
    assert!(result.backup_path.is_none());
    assert!(
        fs::read_to_string(home.join(".cc-switch/skills/update-check/SKILL.md"))
            .expect("read unchanged Library snapshot")
            .contains("old snapshot")
    );
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .expect("read Library row")
            .expect("Library row remains")
            .content_hash,
        skill.content_hash
    );
}

#[test]
fn disappeared_library_row_during_update_is_reinserted_before_rolled_back() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    state
        .db
        .disappear_library_skill_on_update_for_test()
        .expect("install row disappearance failpoint");
    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token: check.stage_token.expect("stage token"),
            observation_token: check.observation_token,
            confirm_local_modifications: false,
        },
    )
    .expect("structured disappeared-row result");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::RolledBack);
    assert!(
        fs::read_to_string(home.join(".cc-switch/skills/update-check/SKILL.md"))
            .expect("read restored snapshot")
            .contains("old snapshot")
    );
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .expect("read restored Library row")
            .expect("Library row restored")
            .content_hash,
        skill.content_hash
    );
}

#[test]
fn disappeared_library_row_during_delete_is_reinserted_before_rolled_back() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-row-disappears");
    write_skill(&source, "retain after row loss");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect deletion");
    state
        .db
        .disappear_library_skill_on_delete_for_test()
        .expect("install row disappearance failpoint");
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("structured disappeared-row result");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::RolledBack
    );
    let destination = home.join(".cc-switch/skills/delete-row-disappears");
    assert!(destination.is_dir());
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_some());
}

#[test]
fn deletion_blocks_when_library_root_is_missing_or_foreign() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    for replacement in ["missing", "file", "symlink"] {
        reset_test_fs();
        let home = ensure_test_home();
        let fixture = tempfile::tempdir().expect("create source fixture");
        let source = fixture.path().join("delete-root-check");
        write_skill(&source, "delete root");
        let state = create_test_state().expect("create test state");
        let skill = LibrarySkillAcquisitionService::acquire_from_directory(
            &state.db,
            &source,
            git_source(),
            None,
        )
        .expect("admit Skill");
        let destination = home.join(".cc-switch/skills/delete-root-check");
        match replacement {
            "missing" => fs::remove_dir_all(&destination).expect("remove Library root"),
            "file" => {
                fs::remove_dir_all(&destination).expect("remove Library root");
                fs::write(&destination, "foreign file").expect("create foreign file");
            }
            "symlink" => {
                fs::remove_dir_all(&destination).expect("remove Library root");
                let foreign = fixture.path().join("foreign");
                write_skill(&foreign, "foreign target");
                #[cfg(unix)]
                std::os::unix::fs::symlink(&foreign, &destination)
                    .expect("create foreign Library symlink");
            }
            _ => unreachable!(),
        }

        let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
            .expect("inspect foreign Library root");
        assert!(plan.blocked, "{replacement} root must block deletion");
        let result = LibrarySkillUpdateService::delete(
            &state.db,
            cc_switch_lib::LibrarySkillDeletionIntent {
                library_skill_id: skill.id.clone(),
                observation_token: plan.observation_token,
            },
        )
        .expect("structured foreign-root block");
        assert_eq!(
            result.outcome,
            cc_switch_lib::LibrarySkillDeletionOutcome::Blocked
        );
        assert!(state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .is_some());
        match replacement {
            "missing" => assert!(!destination.exists()),
            "file" | "symlink" => assert!(destination.symlink_metadata().is_ok()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn deletion_undeploy_error_restores_exact_link_even_when_first_compensation_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-compensation");
    write_skill(&source, "delete compensation");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let target = DeploymentTarget::global(DeploymentConsumer::Claude);
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy Skill");
    state
        .db
        .fail_skill_deployment_deletes_for_test()
        .expect("install deployment delete failpoint");
    SkillDeploymentService::force_compensation_failure_for_test(true);

    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect deletion");
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("structured undeploy compensation");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::RolledBack
    );
    assert!(result.items.iter().any(|item| {
        item.outcome == DeploymentMutationOutcome::Error
            && item
                .message
                .as_deref()
                .is_some_and(|message| message.contains("database deletion failed"))
    }));
    assert!(home.join(".claude/skills/delete-compensation").is_symlink());
    assert!(state
        .db
        .get_skill_deployment(&skill.id, &target)
        .unwrap()
        .is_some());
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_some());
}

#[test]
fn deletion_rename_and_restore_failure_keeps_source_and_requires_recovery() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-rename-failure");
    write_skill(&source, "retain source");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let target = DeploymentTarget::global(DeploymentConsumer::Claude);
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy Skill");

    LibrarySkillUpdateService::force_deletion_rename_failure_for_test(true);
    SkillDeploymentService::force_compensation_failure_for_test(true);
    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect deletion");
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("structured rename/restore failure");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::RecoveryRequired
    );
    assert!(result.backup_path.is_none());
    assert!(result
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Library source remained in place")));
    let destination = home.join(".cc-switch/skills/delete-rename-failure");
    assert!(
        destination.is_dir(),
        "failed rename must leave source in place"
    );
    assert!(fs::read_to_string(destination.join("SKILL.md"))
        .expect("read retained source")
        .contains("retain source"));
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_some());
    assert!(state
        .db
        .get_skill_deployment(&skill.id, &target)
        .unwrap()
        .is_none());
    let backups = home.join(".cc-switch/skill-import-backups");
    assert!(
        !backups.exists()
            || fs::read_dir(backups)
                .expect("read backup root")
                .next()
                .is_none(),
        "an empty backup root must not be reported as recoverable"
    );
}

#[test]
fn library_delete_db_error_with_restore_failure_has_no_empty_backup_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-db-error");
    write_skill(&source, "retain on db error");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let target = DeploymentTarget::global(DeploymentConsumer::Claude);
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy Skill");
    state
        .db
        .fail_library_skill_deletes_for_test()
        .expect("install Library delete failpoint");
    SkillDeploymentService::force_compensation_failure_for_test(true);
    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect deletion");
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("structured DB/restore failure");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::RecoveryRequired
    );
    assert!(result.backup_path.is_none());
    assert!(result
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Library source remains in place")));
    assert!(home.join(".cc-switch/skills/delete-db-error").is_dir());
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_some());
    assert!(state
        .db
        .get_skill_deployment(&skill.id, &target)
        .unwrap()
        .is_none());
}

#[test]
fn update_backups_are_pruned_to_twenty_managed_roots() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "snapshot 0");
    write_skill(&upstream, "snapshot 1");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");

    for index in 1..=21 {
        write_skill(&upstream, &format!("snapshot {index}"));
        let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
            &state.db,
            &skill.id,
            fixture.path().join("upstream").as_path(),
        )
        .expect("stage update snapshot");
        let result = LibrarySkillUpdateService::apply(
            &state.db,
            LibrarySkillUpdateApplyIntent {
                library_skill_id: skill.id.clone(),
                stage_token: check.stage_token.expect("stage token"),
                observation_token: check.observation_token,
                confirm_local_modifications: false,
            },
        )
        .expect("apply update snapshot");
        assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Updated);
    }
    let backup_root = home.join(".cc-switch/skill-import-backups");
    let count = fs::read_dir(&backup_root)
        .expect("read managed backup roots")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|metadata| metadata.is_dir()))
        .count();
    assert_eq!(
        count, 20,
        "managed backup retention should keep exactly 20 roots"
    );
    for entry in fs::read_dir(&backup_root)
        .expect("read retained managed backup roots")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|metadata| metadata.is_dir()))
    {
        assert!(
            entry.path().join("library-old/SKILL.md").is_file(),
            "retained backup {} must contain a recoverable Library snapshot",
            entry.path().display()
        );
    }
}

#[test]
fn staged_update_applies_atomically_and_keeps_a_managed_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");

    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    assert_eq!(
        check.outcome,
        LibrarySkillUpdateCheckOutcome::UpdateAvailable
    );
    let stage_token = check.stage_token.clone().expect("stage token");
    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token,
            observation_token: check.observation_token,
            confirm_local_modifications: false,
        },
    )
    .expect("apply staged update");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Updated);
    assert!(
        fs::read_to_string(home.join(".cc-switch/skills/update-check/SKILL.md"))
            .expect("read updated Library snapshot")
            .contains("new snapshot")
    );
    assert!(result
        .backup_path
        .as_deref()
        .map(std::path::Path::new)
        .is_some_and(Path::exists));
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Update),
            ..ActivityQuery::default()
        })
        .expect("list update activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Success);
}

#[test]
fn local_modification_requires_confirmation_before_replacement() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create update fixture");
    let local = fixture.path().join("local");
    let upstream = fixture.path().join("upstream/update-check");
    write_skill(&local, "old snapshot");
    write_skill(&upstream, "new snapshot");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &local,
        git_source(),
        Some("update-check"),
    )
    .expect("admit initial Library snapshot");
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: DeploymentTarget::global(DeploymentConsumer::Claude),
        }))
        .expect("deploy through Claude link");
    fs::write(
        home.join(".claude/skills/update-check/SKILL.md"),
        "---\nname: update-check\ndescription: Update check fixture\n---\n\nlocal edit\n",
    )
    .expect("modify live Library snapshot");

    let check = LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        fixture.path().join("upstream").as_path(),
    )
    .expect("stage upstream update");
    assert!(check.local_modified);
    let result = LibrarySkillUpdateService::apply(
        &state.db,
        LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token: check.stage_token.clone().expect("stage token"),
            observation_token: check.observation_token.clone(),
            confirm_local_modifications: false,
        },
    )
    .expect("structured local-modification block");
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Blocked);
    assert_eq!(
        result.reason,
        Some(LibrarySkillUpdateReason::LocalModificationConfirmationRequired)
    );
    assert!(
        fs::read_to_string(home.join(".cc-switch/skills/update-check/SKILL.md"))
            .expect("live Library remains")
            .contains("local edit")
    );
}

#[test]
fn deletion_removes_exact_link_and_library_snapshot_with_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-check");
    write_skill(&source, "delete me");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let target = DeploymentTarget::global(DeploymentConsumer::Claude);
    let deployment = SkillDeploymentService::new(state.db.clone());
    let applied = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy Skill");
    assert_eq!(applied.items[0].outcome, DeploymentMutationOutcome::Applied);
    fs::write(
        home.join(".cc-switch/skills/delete-check/SKILL.md"),
        "---\nname: update-check\ndescription: Update check fixture\n---\n\nlocal deletion edit\n",
    )
    .expect("make local deletion edit");

    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect deletion");
    assert!(!plan.blocked);
    assert_eq!(plan.targets.len(), 1);
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("delete Library Skill");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::Deleted
    );
    let backup = result
        .backup_path
        .as_deref()
        .map(Path::new)
        .expect("managed deletion backup path");
    assert!(backup.is_dir());
    assert!(fs::read_to_string(backup.join("library-deleted/SKILL.md"))
        .expect("read local deletion backup")
        .contains("local deletion edit"));
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_none());
    assert!(!home.join(".claude/skills/delete-check").exists());
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            library_skill_id: Some(skill.id.clone()),
            ..ActivityQuery::default()
        })
        .expect("list deletion activity");
    let removal = activity
        .entries
        .iter()
        .find(|entry| {
            entry.operation == ActivityOperation::Removal
                && entry.reason == ActivityReason::LibraryRemove
        })
        .expect("Library removal row must survive deletion");
    assert_eq!(removal.outcome, ActivityOutcome::Success);
    let child = activity
        .entries
        .iter()
        .find(|entry| entry.reason == ActivityReason::DeploymentRemove)
        .expect("deletion child row");
    assert_eq!(child.outcome, ActivityOutcome::Success);
    assert_eq!(child.target.consumer, Some(DeploymentConsumer::Claude));
    assert_eq!(child.target.workspace_kind, Some(WorkspaceKind::Global));
    assert!(child.target.workspace_id.is_none());
}

#[test]
fn deletion_removes_safe_links_but_blocks_on_drifted_targets() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixture = tempfile::tempdir().expect("create source fixture");
    let source = fixture.path().join("delete-mixed");
    write_skill(&source, "delete mixed");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        git_source(),
        None,
    )
    .expect("admit Skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    for target in [
        DeploymentTarget::global(DeploymentConsumer::Claude),
        DeploymentTarget::global(DeploymentConsumer::Codex),
    ] {
        deployment
            .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
                library_skill_id: skill.id.clone(),
                target,
            }))
            .expect("deploy target");
    }
    let codex_link = home.join(".agents/skills/delete-mixed");
    fs::remove_file(&codex_link).expect("remove Codex link");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/tmp/foreign-delete-target", &codex_link)
        .expect("create foreign Codex link");

    let plan = LibrarySkillUpdateService::inspect_deletion(&state.db, &skill.id)
        .expect("inspect mixed deletion");
    assert!(plan.blocked);
    let result = LibrarySkillUpdateService::delete(
        &state.db,
        cc_switch_lib::LibrarySkillDeletionIntent {
            library_skill_id: skill.id.clone(),
            observation_token: plan.observation_token,
        },
    )
    .expect("structured mixed deletion block");
    assert_eq!(
        result.outcome,
        cc_switch_lib::LibrarySkillDeletionOutcome::Blocked
    );
    assert!(!home.join(".claude/skills/delete-mixed").exists());
    assert!(home.join(".agents/skills/delete-mixed").is_symlink());
    assert!(state
        .db
        .get_library_skill_by_id(&skill.id)
        .unwrap()
        .is_some());
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 1);
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Removal),
            library_skill_id: Some(skill.id.clone()),
            ..ActivityQuery::default()
        })
        .expect("list mixed deletion activity");
    let final_row = activity
        .entries
        .iter()
        .find(|entry| entry.reason == ActivityReason::LibraryRemove)
        .expect("final Library removal activity");
    assert_eq!(final_row.outcome, ActivityOutcome::Blocked);
}
