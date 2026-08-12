#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;

use cc_switch_lib::{
    ActivityOperation, ActivityQuery, ActivityReason, ActivityRecorder, DeploymentBatch,
    DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome, DeploymentRecoveryDisposition,
    DeploymentRecoveryQuery, DeploymentRecoveryReason, DeploymentRecoveryService, DeploymentTarget,
    LibrarySkillAcquisitionService, LibrarySkillSource, LibrarySourceKind, ProjectWorkspace,
    WorkspaceKind, WorkspaceLifecycle, WorkspaceRootKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(path: &std::path::Path, name: &str) {
    fs::create_dir_all(path).expect("create Skill tree");
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: recovery fixture\n---\n\nBody.\n"),
    )
    .expect("write Skill manifest");
}

fn source() -> LibrarySkillSource {
    LibrarySkillSource {
        kind: LibrarySourceKind::Zip,
        url: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        skill_path: None,
        marketplace: None,
    }
}

fn acquire(
    db: &std::sync::Arc<cc_switch_lib::Database>,
    parent: &std::path::Path,
    directory: &str,
) -> cc_switch_lib::LibrarySkill {
    let path = parent.join(directory);
    write_skill(&path, directory);
    LibrarySkillAcquisitionService::acquire_from_directory(db, &path, source(), None)
        .expect("acquire Library Skill")
}

fn global(consumer: DeploymentConsumer) -> DeploymentTarget {
    DeploymentTarget::global(consumer)
}

fn non_git_fingerprint(root: &std::path::Path) -> String {
    let metadata = fs::metadata(root).expect("read project root metadata");
    format!("non_git:v2:{}:{}", metadata.dev(), metadata.ino())
}

fn run_git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-C", &root.display().to_string()])
        .args(args)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git command failed: {:?}", args);
}

#[test]
fn inspection_proposes_only_exact_unrecorded_library_links_without_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "recover-exact");
    let consumer_root = home.join(".claude/skills");
    fs::create_dir_all(&consumer_root).expect("create consumer root");
    let link = consumer_root.join("recover-exact");
    let library_path = home.join(".cc-switch/skills/recover-exact");
    std::os::unix::fs::symlink(&library_path, &link).expect("create unrecorded exact link");

    let before_link = fs::read_link(&link).expect("read link before inspection");
    let inspected = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .expect("inspect recovery");

    assert_eq!(inspected.findings.len(), 1);
    let finding = &inspected.findings[0];
    assert_eq!(
        finding.disposition,
        DeploymentRecoveryDisposition::Recoverable
    );
    assert_eq!(
        finding.safe_reason,
        Some(DeploymentRecoveryReason::ExactLibraryLink)
    );
    assert_eq!(finding.library_skill_id.as_deref(), Some(skill.id.as_str()));
    assert_eq!(finding.entry_name, "recover-exact");
    assert_eq!(
        finding.observed_target.as_deref(),
        Some(library_path.to_str().unwrap())
    );
    assert!(finding.observation_token.is_some());
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::read_link(&link).unwrap(), before_link);
}

#[test]
fn inspection_separates_unsafe_links_and_existing_desired_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let exact = acquire(&state.db, source_root.path(), "exact");
    let desired = acquire(&state.db, source_root.path(), "desired");
    let root = home.join(".claude/skills");
    fs::create_dir_all(&root).expect("create consumer root");
    let library = home.join(".cc-switch/skills");
    std::os::unix::fs::symlink(library.join("exact"), root.join("alias"))
        .expect("create ambiguous alias");
    std::os::unix::fs::symlink("../../.cc-switch/skills/exact", root.join("relative"))
        .expect("create relative ambiguous link");
    std::os::unix::fs::symlink(home.join("missing-target"), root.join("broken"))
        .expect("create broken link");
    let outside = tempfile::tempdir().expect("create outside target");
    std::os::unix::fs::symlink(outside.path(), root.join("foreign")).expect("create foreign link");
    std::os::unix::fs::symlink(outside.path(), root.join("exact"))
        .expect("create foreign link with known Library basename");
    fs::create_dir_all(library.join("not-recorded")).expect("create non-Library directory");
    std::os::unix::fs::symlink(library.join("not-recorded"), root.join("not-recorded"))
        .expect("create non-Library link");
    fs::write(root.join("occupied"), "user content").expect("create occupied file");
    std::os::unix::fs::symlink(library.join("desired"), root.join("desired"))
        .expect("create desired link");
    state
        .db
        .save_skill_deployment(&cc_switch_lib::DesiredDeployment {
            id: uuid::Uuid::new_v4().to_string(),
            library_skill_id: desired.id,
            library_directory: "desired".to_string(),
            target: global(DeploymentConsumer::Claude),
            created_at: 1,
            updated_at: 1,
        })
        .expect("save desired row");

    // A lexical Library child whose canonical target escapes is never adoptable.
    let escaping = library.join("escaping");
    std::os::unix::fs::symlink(outside.path(), &escaping).expect("create escaping Library link");
    std::os::unix::fs::symlink(&escaping, root.join("escaping"))
        .expect("create consumer escaping link");

    let result = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .expect("inspect unsafe links");
    let disposition = |name: &str| {
        result
            .findings
            .iter()
            .find(|finding| finding.entry_name == name)
            .unwrap_or_else(|| panic!("missing finding {name}"))
            .disposition
    };
    assert_eq!(
        disposition("alias"),
        DeploymentRecoveryDisposition::AmbiguousLink
    );
    assert_eq!(
        disposition("relative"),
        DeploymentRecoveryDisposition::AmbiguousLink
    );
    assert_eq!(
        disposition("broken"),
        DeploymentRecoveryDisposition::BrokenLink
    );
    assert_eq!(
        disposition("foreign"),
        DeploymentRecoveryDisposition::ForeignLink
    );
    assert_eq!(
        disposition("exact"),
        DeploymentRecoveryDisposition::ForeignLink
    );
    assert_eq!(
        disposition("not-recorded"),
        DeploymentRecoveryDisposition::NonLibrary
    );
    assert_eq!(
        disposition("occupied"),
        DeploymentRecoveryDisposition::Occupied
    );
    assert_eq!(
        disposition("escaping"),
        DeploymentRecoveryDisposition::EscapingLink
    );
    assert_eq!(
        disposition("desired"),
        DeploymentRecoveryDisposition::DesiredExists
    );
    assert!(!result.findings.iter().any(|finding| {
        finding.library_skill_id.as_deref() == Some(exact.id.as_str())
            && finding.disposition == DeploymentRecoveryDisposition::Recoverable
    }));
}

#[test]
fn confirmed_recovery_revalidates_and_records_without_replacing_the_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "confirm-recovery");
    let root = home.join(".claude/skills");
    fs::create_dir_all(&root).expect("create consumer root");
    let link = root.join("confirm-recovery");
    let expected = home.join(".cc-switch/skills/confirm-recovery");
    std::os::unix::fs::symlink(&expected, &link).expect("create exact link");
    let metadata_before = fs::symlink_metadata(&link).expect("inspect link before");
    let service = DeploymentRecoveryService::new(state.db.clone());
    let proposal = service
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .unwrap()
        .findings
        .into_iter()
        .find(|finding| finding.entry_name == "confirm-recovery")
        .expect("find proposal");

    // Presentation metadata is not physical Deployment identity and must not
    // invalidate an otherwise exact observation.
    state
        .db
        .update_library_skill_display_metadata(&skill.id, "Renamed after scan", None, 99)
        .expect("rename display metadata");

    let rejected = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id.clone(),
            target: global(DeploymentConsumer::Claude),
            observation_token: proposal.observation_token.clone().unwrap(),
            confirmed: false,
        }))
        .expect("reject recovery");
    assert_eq!(
        rejected.items[0].outcome,
        DeploymentMutationOutcome::Blocked
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());

    let applied = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id.clone(),
            target: global(DeploymentConsumer::Claude),
            observation_token: proposal.observation_token.unwrap(),
            confirmed: true,
        }))
        .expect("confirm recovery");
    assert_eq!(applied.items[0].outcome, DeploymentMutationOutcome::Applied);
    assert_eq!(fs::read_link(&link).unwrap(), expected);
    let metadata_after = fs::symlink_metadata(&link).expect("inspect link after");
    assert_eq!(
        metadata_before.ino(),
        metadata_after.ino(),
        "link was replaced"
    );
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 1);

    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Deployment),
            reason: Some(ActivityReason::RecoverDeployment),
            ..ActivityQuery::default()
        })
        .expect("list recovery activity");
    assert_eq!(
        activity.entries.len(),
        2,
        "one row for each confirmed/rejected item"
    );
}

#[test]
fn physical_library_replacement_invalidates_a_recovery_observation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "physical-change");
    let root = home.join(".claude/skills");
    fs::create_dir_all(&root).unwrap();
    let library_path = home.join(".cc-switch/skills/physical-change");
    let link = root.join("physical-change");
    std::os::unix::fs::symlink(&library_path, &link).unwrap();
    let proposal = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .unwrap()
        .findings
        .into_iter()
        .find(|finding| finding.entry_name == "physical-change")
        .unwrap();

    fs::remove_dir_all(&library_path).expect("remove old Library directory");
    write_skill(&library_path, "physical-change");
    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target: global(DeploymentConsumer::Claude),
            observation_token: proposal.observation_token.unwrap(),
            confirmed: true,
        }))
        .expect("apply stale physical observation");
    assert_eq!(
        result.items[0].outcome,
        DeploymentMutationOutcome::StaleObservation
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::read_link(link).unwrap(), library_path);
}

#[test]
fn removed_library_row_makes_confirmation_stale_without_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "removed-row");
    let root = home.join(".agents/skills");
    fs::create_dir_all(&root).unwrap();
    let link = root.join("removed-row");
    let library = home.join(".cc-switch/skills/removed-row");
    std::os::unix::fs::symlink(&library, &link).unwrap();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Codex),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    state.db.delete_library_skill(&skill.id).unwrap();

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target: global(DeploymentConsumer::Codex),
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    assert_eq!(
        result.items[0].outcome,
        DeploymentMutationOutcome::StaleObservation
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::read_link(link).unwrap(), library);
}

#[test]
fn project_becoming_unavailable_after_scan_blocks_confirmation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let project = tempfile::tempdir().expect("create project");
    let project_path = project.path().to_path_buf();
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "became-unavailable");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(&project_path, None)
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = project_path.join(".claude/skills/became-unavailable");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/became-unavailable"), &link).unwrap();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    drop(project);

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Blocked);
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
}

#[test]
fn recovery_revalidates_workspace_immediately_before_first_side_effect() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "late-archive");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(repository.path(), None)
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = repository.path().join(".claude/skills/late-archive");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/late-archive"), &link).unwrap();
    let inode = fs::symlink_metadata(&link).unwrap().ino();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    cc_switch_lib::SkillDeploymentService::force_recovery_workspace_archive_before_commit_for_test(
        true,
    );

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    cc_switch_lib::SkillDeploymentService::force_recovery_workspace_archive_before_commit_for_test(
        false,
    );
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Blocked);
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::symlink_metadata(&link).unwrap().ino(), inode);
    assert!(
        !fs::read_to_string(repository.path().join(".git/info/exclude"))
            .unwrap_or_default()
            .contains("late-archive")
    );
}

#[test]
fn active_project_recovery_adds_git_exclude_without_replacing_the_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "project-recovery");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(repository.path(), Some("Recovery project".to_string()))
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Codex,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = repository.path().join(".agents/skills/project-recovery");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/project-recovery"), &link).unwrap();
    let inode = fs::symlink_metadata(&link).unwrap().ino();
    let proposal = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Codex),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings
        .into_iter()
        .next()
        .expect("project proposal");

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: proposal.observation_token.unwrap(),
            confirmed: true,
        }))
        .unwrap();
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Applied);
    assert_eq!(fs::symlink_metadata(&link).unwrap().ino(), inode);
    assert!(
        fs::read_to_string(repository.path().join(".git/info/exclude"))
            .unwrap()
            .contains("/.agents/skills/project-recovery # cc-switch managed")
    );
}

#[test]
fn project_recovery_rolls_back_git_exclude_when_database_save_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "rollback-recovery");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(repository.path(), None)
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = repository.path().join(".claude/skills/rollback-recovery");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/rollback-recovery"), &link).unwrap();
    let inode = fs::symlink_metadata(&link).unwrap().ino();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    state.db.fail_skill_deployment_inserts_for_test().unwrap();

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Error);
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::symlink_metadata(&link).unwrap().ino(), inode);
    assert!(
        !fs::read_to_string(repository.path().join(".git/info/exclude"))
            .unwrap_or_default()
            .contains("rollback-recovery")
    );
}

#[test]
fn project_recovery_exclude_write_failure_leaves_link_and_desired_state_unchanged() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "exclude-failure-recovery");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(repository.path(), None)
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Codex,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = repository
        .path()
        .join(".agents/skills/exclude-failure-recovery");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/exclude-failure-recovery"),
        &link,
    )
    .unwrap();
    let inode = fs::symlink_metadata(&link).unwrap().ino();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Codex),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    let exclude = repository.path().join(".git/info/exclude");
    fs::remove_file(&exclude).unwrap();
    fs::create_dir(&exclude).unwrap();

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Error);
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(fs::symlink_metadata(&link).unwrap().ino(), inode);
    assert!(
        exclude.is_dir(),
        "foreign exclude fixture must remain untouched"
    );
}

#[test]
fn failed_project_exclude_compensation_reports_recovery_required() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "compensation-recovery");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(repository.path(), None)
        .unwrap()
        .workspace;
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id,
    };
    let link = repository
        .path()
        .join(".claude/skills/compensation-recovery");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/compensation-recovery"), &link)
        .unwrap();
    let token = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: Some(target.workspace_id.clone()),
        })
        .unwrap()
        .findings[0]
        .observation_token
        .clone()
        .unwrap();
    state.db.fail_skill_deployment_inserts_for_test().unwrap();
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(true);

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Recover {
            library_skill_id: skill.id,
            target,
            observation_token: token,
            confirmed: true,
        }))
        .unwrap();
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(false);
    assert_eq!(
        result.items[0].outcome,
        DeploymentMutationOutcome::RecoveryRequired
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert!(
        fs::read_to_string(repository.path().join(".git/info/exclude"))
            .unwrap_or_default()
            .contains("compensation-recovery")
    );
}

#[test]
fn stale_and_partial_batch_recovery_leave_each_rejected_item_unchanged() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let first = acquire(&state.db, source_root.path(), "first-recovery");
    let second = acquire(&state.db, source_root.path(), "second-recovery");
    let root = home.join(".agents/skills");
    fs::create_dir_all(&root).expect("create consumer root");
    for directory in ["first-recovery", "second-recovery"] {
        std::os::unix::fs::symlink(
            home.join(".cc-switch/skills").join(directory),
            root.join(directory),
        )
        .expect("create exact link");
    }
    let proposals = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Codex),
            workspace: Some(WorkspaceKind::Global),
            workspace_id: None,
        })
        .unwrap();
    let token = |name: &str| {
        proposals
            .findings
            .iter()
            .find(|finding| finding.entry_name == name)
            .and_then(|finding| finding.observation_token.clone())
            .unwrap()
    };
    fs::remove_file(root.join("second-recovery")).expect("remove second exact link");
    let foreign = tempfile::tempdir().expect("create replacement target");
    std::os::unix::fs::symlink(foreign.path(), root.join("second-recovery"))
        .expect("replace second link");

    let result = cc_switch_lib::SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch {
            intents: vec![
                DeploymentIntent::Recover {
                    library_skill_id: first.id.clone(),
                    target: global(DeploymentConsumer::Codex),
                    observation_token: token("first-recovery"),
                    confirmed: true,
                },
                DeploymentIntent::Recover {
                    library_skill_id: second.id.clone(),
                    target: global(DeploymentConsumer::Codex),
                    observation_token: token("second-recovery"),
                    confirmed: true,
                },
            ],
        })
        .expect("apply partial recovery batch");
    assert_eq!(result.items[0].library_skill_id, first.id);
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Applied);
    assert_eq!(result.items[1].library_skill_id, second.id);
    assert_eq!(
        result.items[1].outcome,
        DeploymentMutationOutcome::StaleObservation
    );
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 1);
    assert_eq!(
        fs::read_link(root.join("second-recovery")).unwrap(),
        foreign.path()
    );
}

#[test]
fn project_lifecycle_and_metadata_rename_are_classified_without_scan_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let state = create_test_state().expect("create state");
    let skill = acquire(&state.db, source_root.path(), "stable-directory");
    state
        .db
        .update_library_skill_display_metadata(&skill.id, "Renamed display", None, 2)
        .expect("rename display metadata");

    let archived_root = tempfile::tempdir().expect("create archived project");
    let archived_id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .save_project_workspace(&ProjectWorkspace {
            id: archived_id.clone(),
            display_name: "Archived".to_string(),
            root_path: fs::canonicalize(archived_root.path()).unwrap(),
            root_kind: WorkspaceRootKind::NonGit,
            registration_fingerprint: non_git_fingerprint(archived_root.path()),
            lifecycle: WorkspaceLifecycle::Archived,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    let archived_link = archived_root.path().join(".claude/skills/stable-directory");
    fs::create_dir_all(archived_link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(
        home.join(".cc-switch/skills/stable-directory"),
        &archived_link,
    )
    .unwrap();

    let unavailable_root = tempfile::tempdir().expect("create unavailable project");
    let unavailable_path = unavailable_root.path().to_path_buf();
    let unavailable_id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .save_project_workspace(&ProjectWorkspace {
            id: unavailable_id.clone(),
            display_name: "Unavailable".to_string(),
            root_path: fs::canonicalize(&unavailable_path).unwrap(),
            root_kind: WorkspaceRootKind::NonGit,
            registration_fingerprint: non_git_fingerprint(&unavailable_path),
            lifecycle: WorkspaceLifecycle::Active,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    drop(unavailable_root);

    let findings = DeploymentRecoveryService::new(state.db.clone())
        .inspect(DeploymentRecoveryQuery {
            consumer: Some(DeploymentConsumer::Claude),
            workspace: Some(WorkspaceKind::Project),
            workspace_id: None,
        })
        .expect("inspect project recovery")
        .findings;
    let archived = findings
        .iter()
        .find(|finding| finding.target.workspace_id == archived_id)
        .expect("archived finding");
    assert_eq!(
        archived.disposition,
        DeploymentRecoveryDisposition::ArchivedWorkspace
    );
    assert_eq!(
        archived.library_skill_id.as_deref(),
        Some(skill.id.as_str())
    );
    let unavailable = findings
        .iter()
        .find(|finding| finding.target.workspace_id == unavailable_id)
        .expect("unavailable scope finding");
    assert_eq!(
        unavailable.disposition,
        DeploymentRecoveryDisposition::UnavailableWorkspace
    );
    assert!(unavailable.library_skill_id.is_none());
    assert_eq!(
        state
            .db
            .get_project_workspace(&unavailable_id)
            .unwrap()
            .unwrap()
            .lifecycle,
        WorkspaceLifecycle::Active,
        "inspection must not persist Active -> Unavailable"
    );
}
