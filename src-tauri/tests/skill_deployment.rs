#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use cc_switch_lib::{
    ConsumerCompatibility, DeploymentBatch, DeploymentConsumer, DeploymentIntent,
    DeploymentMutationOutcome, DeploymentQuery, DeploymentStatus, DeploymentTarget,
    DesiredDeployment, LibrarySkill, LibrarySkillAcquisitionService, LibrarySkillCompatibility,
    LibrarySkillSource, LibrarySourceKind, ProjectWorkspace, ProjectWorkspaceService,
    SkillDeploymentService, WorkspaceKind, WorkspaceLifecycle, WorkspaceRootKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path) {
    fs::create_dir_all(dir).expect("create skill tree");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: deploy-review\ndescription: Deploy review workflow\n---\n\nReview carefully.\n",
    )
    .expect("write manifest");
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

fn target() -> DeploymentTarget {
    DeploymentTarget::global(DeploymentConsumer::Claude)
}

fn codex_target() -> DeploymentTarget {
    DeploymentTarget::global(DeploymentConsumer::Codex)
}

#[test]
fn inspect_apply_deploys_an_absolute_global_claude_link_idempotently() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("deploy-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    let query = DeploymentQuery::for_target(target());

    let before = deployment.inspect(query.clone()).expect("inspect before");
    let before_item = before
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("Library skill must be inspectable");
    assert!(before_item.desired.is_none());
    assert_eq!(before_item.status, DeploymentStatus::NotDeployed);

    let batch = DeploymentBatch::single(DeploymentIntent::Deploy {
        library_skill_id: skill.id.clone(),
        target: target(),
    });
    let applied = deployment.apply(batch).expect("apply deployment");
    assert_eq!(applied.items.len(), 1);
    assert_eq!(applied.items[0].outcome, DeploymentMutationOutcome::Applied);

    let link = home.join(".claude/skills/deploy-review");
    let link_target = fs::read_link(&link).expect("read deployed link");
    assert!(
        link_target.is_absolute(),
        "deployment target must be absolute"
    );
    assert_eq!(link_target, home.join(".cc-switch/skills/deploy-review"));
    assert!(fs::symlink_metadata(home.join(".claude/skills"))
        .expect("inspect target root")
        .is_dir());

    let inspected = deployment.inspect(query).expect("inspect after");
    let item = inspected
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("deployed Library skill must be inspectable");
    assert_eq!(item.status, DeploymentStatus::InSync);
    assert_eq!(
        item.observed.state,
        cc_switch_lib::ObservedDeploymentState::CorrectLink
    );
    assert!(item.desired.is_some());

    let repeated = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id,
            target: target(),
        }))
        .expect("repeated deploy");
    assert_eq!(
        repeated.items[0].outcome,
        DeploymentMutationOutcome::AlreadyInSync
    );
    assert!(link.is_symlink());
    assert_eq!(
        fs::read_dir(home.join(".claude/skills")).unwrap().count(),
        1
    );
}

#[test]
fn undeploy_only_removes_the_expected_managed_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("undeploy-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    let target = target();
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy skill");

    let removed = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Undeploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("undeploy skill");
    assert_eq!(removed.items[0].outcome, DeploymentMutationOutcome::Removed);
    assert!(!home.join(".claude/skills/undeploy-review").exists());
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
}

#[test]
fn occupied_paths_are_reported_without_overwrite() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("occupied-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let target_root = home.join(".claude/skills");
    fs::create_dir_all(&target_root).expect("create Claude root");
    fs::write(target_root.join("occupied-review"), "user data").expect("occupy target");
    let deployment = SkillDeploymentService::new(state.db.clone());

    let result = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target(),
        }))
        .expect("occupied path is an expected outcome");
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Conflict);
    assert_eq!(
        deployment
            .inspect(DeploymentQuery::for_target(target()))
            .unwrap()
            .items
            .iter()
            .find(|item| item.library_skill_id == skill.id)
            .unwrap()
            .status,
        DeploymentStatus::Conflict
    );
    assert_eq!(
        fs::read_to_string(target_root.join("occupied-review")).unwrap(),
        "user data"
    );
}

#[test]
fn codex_global_uses_official_agents_root_without_legacy_codex_writes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("codex-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());

    let result = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: codex_target(),
        }))
        .expect("deploy Codex skill");
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Applied);

    let link = home.join(".agents/skills/codex-review");
    assert!(link.is_symlink());
    assert!(fs::read_link(&link).unwrap().is_absolute());
    assert_eq!(
        fs::read_link(&link).unwrap(),
        home.join(".cc-switch/skills/codex-review")
    );
    assert!(!home.join(".codex").exists());

    let inspected = deployment
        .inspect(DeploymentQuery::for_target(codex_target()))
        .expect("inspect Codex deployment");
    let item = inspected
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .unwrap();
    assert_eq!(item.status, DeploymentStatus::InSync);

    let repeated = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: codex_target(),
        }))
        .expect("repeat Codex deployment");
    assert_eq!(
        repeated.items[0].outcome,
        DeploymentMutationOutcome::AlreadyInSync
    );

    let removed = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Undeploy {
            library_skill_id: skill.id.clone(),
            target: codex_target(),
        }))
        .expect("undeploy Codex skill");
    assert_eq!(removed.items[0].outcome, DeploymentMutationOutcome::Removed);
    assert!(!link.exists());

    fs::write(&link, "user data").expect("occupy Codex target");
    let conflict = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id,
            target: codex_target(),
        }))
        .expect("occupied Codex target is an expected outcome");
    assert_eq!(
        conflict.items[0].outcome,
        DeploymentMutationOutcome::Conflict
    );
    assert_eq!(fs::read_to_string(&link).unwrap(), "user data");
    assert!(!home.join(".codex").exists());
}

#[test]
fn claude_and_codex_desired_state_is_independent_for_one_library_skill() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("dual-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());

    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target(),
        }))
        .expect("deploy Claude skill");
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: codex_target(),
        }))
        .expect("deploy Codex skill");

    assert!(home.join(".claude/skills/dual-review").is_symlink());
    assert!(home.join(".agents/skills/dual-review").is_symlink());
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 2);

    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Undeploy {
            library_skill_id: skill.id,
            target: codex_target(),
        }))
        .expect("undeploy Codex only");
    assert!(home.join(".claude/skills/dual-review").is_symlink());
    assert!(!home.join(".agents/skills/dual-review").exists());
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 1);
}

#[test]
fn incompatible_consumer_returns_structured_blocked_outcome_without_rewriting_source() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_dir = home.join(".cc-switch/skills/claude-only");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let now = chrono::Utc::now().timestamp();
    let skill = LibrarySkill {
        id: uuid::Uuid::new_v4().to_string(),
        directory: "claude-only".to_string(),
        display_name: "Claude only".to_string(),
        description: Some("Claude-only fixture".to_string()),
        source: source(),
        compatibility: LibrarySkillCompatibility {
            claude: ConsumerCompatibility {
                compatible: true,
                issues: vec![],
            },
            codex: ConsumerCompatibility {
                compatible: false,
                issues: vec!["Codex requires a different canonical contract".to_string()],
            },
        },
        content_hash: "claude-only-hash".to_string(),
        acquired_at: now,
        updated_at: now,
    };
    state.db.save_library_skill(&skill).expect("save fixture");
    let deployment = SkillDeploymentService::new(state.db.clone());

    let inspected = deployment
        .inspect(DeploymentQuery::for_target(codex_target()))
        .expect("inspect incompatible target");
    let inspected_item = inspected
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("incompatible inspection item");
    assert_eq!(inspected_item.status, DeploymentStatus::Blocked);
    assert_eq!(
        inspected_item.observed.state,
        cc_switch_lib::ObservedDeploymentState::Missing
    );

    let result = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id,
            target: codex_target(),
        }))
        .expect("incompatibility is an expected item outcome");
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Blocked);
    assert!(result.items[0]
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("Codex"));
    assert!(!home.join(".agents").exists());
    assert!(!home.join(".codex").exists());
}

#[test]
fn database_insert_failure_compensates_the_new_filesystem_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("insert-failure");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    state
        .db
        .fail_skill_deployment_inserts_for_test()
        .expect("install insert failure trigger");

    let deployment = SkillDeploymentService::new(state.db.clone());
    let result = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id,
            target: target(),
        }))
        .expect("item-level database failure must be reported as a result");
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Error);
    assert!(
        !home.join(".claude/skills/insert-failure").exists(),
        "a failed desired-state insert must remove its newly-created link"
    );
}

#[test]
fn database_delete_failure_recreates_the_removed_filesystem_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("delete-failure");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    let target = target();
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy skill");
    state
        .db
        .fail_skill_deployment_deletes_for_test()
        .expect("install delete failure trigger");

    let result = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Undeploy {
            library_skill_id: skill.id.clone(),
            target,
        }))
        .expect("item-level database failure must be a result");
    assert_eq!(result.items[0].outcome, DeploymentMutationOutcome::Error);
    assert!(home.join(".claude/skills/delete-failure").is_symlink());
    assert_eq!(state.db.list_skill_deployments().unwrap().len(), 1);
}

#[test]
fn inspect_classifies_drift_states_without_persisting_or_repairing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("inspect-matrix");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    let target = target();
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy fixture");
    let desired_before = state.db.list_skill_deployments().expect("snapshot desired");
    let link = home.join(".claude/skills/inspect-matrix");
    let foreign_root = tempfile::tempdir().expect("create foreign target");

    fs::remove_file(&link).expect("remove managed link");
    let missing = deployment
        .inspect(DeploymentQuery::for_target(target.clone()))
        .expect("inspect missing link");
    let missing_item = missing
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("missing item");
    assert_eq!(
        missing_item.observed.state,
        cc_switch_lib::ObservedDeploymentState::Missing
    );
    assert_eq!(missing_item.status, DeploymentStatus::Drift);

    std::os::unix::fs::symlink(foreign_root.path(), &link).expect("create foreign link");
    let redirected = deployment
        .inspect(DeploymentQuery::for_target(target.clone()))
        .expect("inspect redirected link");
    let redirected_item = redirected
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("redirected item");
    assert_eq!(
        redirected_item.observed.state,
        cc_switch_lib::ObservedDeploymentState::RedirectedLink
    );
    assert_eq!(redirected_item.status, DeploymentStatus::Conflict);

    fs::remove_file(&link).expect("remove foreign link");
    std::os::unix::fs::symlink("relative-target", &link).expect("create invalid link");
    let invalid = deployment
        .inspect(DeploymentQuery::for_target(target.clone()))
        .expect("inspect invalid link");
    let invalid_item = invalid
        .items
        .iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("invalid item");
    assert_eq!(
        invalid_item.observed.state,
        cc_switch_lib::ObservedDeploymentState::InvalidLink
    );
    assert_eq!(invalid_item.status, DeploymentStatus::Drift);

    fs::remove_file(&link).expect("remove invalid link");
    fs::create_dir(&link).expect("occupy target with directory");
    let occupied_directory = deployment
        .inspect(DeploymentQuery::for_target(target.clone()))
        .expect("inspect occupied directory");
    assert_eq!(
        occupied_directory
            .items
            .iter()
            .find(|item| item.library_skill_id == skill.id)
            .expect("occupied directory item")
            .observed
            .state,
        cc_switch_lib::ObservedDeploymentState::OccupiedDirectory
    );

    fs::remove_dir(&link).expect("remove occupied directory");
    fs::write(&link, "user-owned").expect("occupy target with file");
    let occupied_file = deployment
        .inspect(DeploymentQuery::for_target(target))
        .expect("inspect occupied file");
    assert_eq!(
        occupied_file
            .items
            .iter()
            .find(|item| item.library_skill_id == skill.id)
            .expect("occupied file item")
            .observed
            .state,
        cc_switch_lib::ObservedDeploymentState::OccupiedFile
    );

    assert_eq!(
        state
            .db
            .list_skill_deployments()
            .expect("desired after inspect"),
        desired_before,
        "inspection must not mutate desired records"
    );
    assert_eq!(
        fs::read_to_string(&link).expect("read occupied file"),
        "user-owned",
        "inspection must not repair or replace observed targets"
    );
}

#[test]
fn inspect_uses_one_matrix_for_global_and_project_consumers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("matrix-review");
    write_skill(&source_dir);
    let project_root = tempfile::tempdir().expect("create project root");
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let workspace = ProjectWorkspaceService::new(state.db.clone())
        .register(project_root.path(), Some("Matrix project".to_string()))
        .expect("register project workspace")
        .workspace;
    let targets = [
        DeploymentTarget::global(DeploymentConsumer::Claude),
        DeploymentTarget::global(DeploymentConsumer::Codex),
        DeploymentTarget {
            consumer: DeploymentConsumer::Claude,
            workspace: WorkspaceKind::Project,
            workspace_id: workspace.id.clone(),
        },
        DeploymentTarget {
            consumer: DeploymentConsumer::Codex,
            workspace: WorkspaceKind::Project,
            workspace_id: workspace.id,
        },
    ];
    let deployment = SkillDeploymentService::new(state.db.clone());

    for target in targets {
        deployment
            .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
                library_skill_id: skill.id.clone(),
                target: target.clone(),
            }))
            .expect("deploy matrix target");
        let target_root = match target.workspace {
            WorkspaceKind::Global => match target.consumer {
                DeploymentConsumer::Claude => home.join(".claude/skills"),
                DeploymentConsumer::Codex => home.join(".agents/skills"),
            },
            WorkspaceKind::Project => match target.consumer {
                DeploymentConsumer::Claude => project_root.path().join(".claude/skills"),
                DeploymentConsumer::Codex => project_root.path().join(".agents/skills"),
            },
        };
        let link = target_root.join(&skill.directory);
        let correct = deployment
            .inspect(DeploymentQuery::for_target(target.clone()))
            .expect("inspect correct matrix target")
            .items
            .into_iter()
            .find(|item| item.library_skill_id == skill.id)
            .expect("correct matrix item");
        assert_eq!(correct.status, DeploymentStatus::InSync);
        assert_eq!(
            correct.observed.state,
            cc_switch_lib::ObservedDeploymentState::CorrectLink
        );

        fs::remove_file(&link).expect("remove matrix link");
        let missing = deployment
            .inspect(DeploymentQuery::for_target(target.clone()))
            .expect("inspect missing matrix target")
            .items
            .into_iter()
            .find(|item| item.library_skill_id == skill.id)
            .expect("missing matrix item");
        assert_eq!(missing.status, DeploymentStatus::Drift);
        assert_eq!(
            missing.observed.state,
            cc_switch_lib::ObservedDeploymentState::Missing
        );

        fs::create_dir(&link).expect("occupy matrix target");
        let occupied = deployment
            .inspect(DeploymentQuery::for_target(target.clone()))
            .expect("inspect occupied matrix target")
            .items
            .into_iter()
            .find(|item| item.library_skill_id == skill.id)
            .expect("occupied matrix item");
        assert_eq!(occupied.status, DeploymentStatus::Conflict);
        assert_eq!(
            occupied.observed.state,
            cc_switch_lib::ObservedDeploymentState::OccupiedDirectory
        );
        fs::remove_dir(&link).expect("remove occupied matrix target");
    }
}

#[test]
fn inspect_classifies_an_inaccessible_consumer_root_as_unreadable() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("unreadable-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());
    let target = target();
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy unreadable fixture");
    let claude_root = home.join(".claude");
    let original_mode = fs::metadata(&claude_root)
        .expect("read consumer root permissions")
        .permissions()
        .mode();
    fs::set_permissions(&claude_root, fs::Permissions::from_mode(0o000))
        .expect("make consumer root unreadable");
    let inspected_result = deployment.inspect(DeploymentQuery::for_target(target));
    fs::set_permissions(&claude_root, fs::Permissions::from_mode(original_mode))
        .expect("restore consumer root permissions");
    let inspected = inspected_result.expect("inspect unreadable consumer root");
    let item = inspected
        .items
        .into_iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("unreadable item");
    assert_eq!(item.status, DeploymentStatus::Drift);
    assert_eq!(
        item.observed.state,
        cc_switch_lib::ObservedDeploymentState::Unreadable
    );
}

#[test]
fn inspect_reports_broken_library_missing_unrecorded_and_lifecycle_states() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("state-review");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        source(),
        None,
    )
    .expect("acquire skill");
    let deployment = SkillDeploymentService::new(state.db.clone());

    let global = DeploymentTarget::global(DeploymentConsumer::Claude);
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: global.clone(),
        }))
        .expect("deploy global fixture");
    let link = home.join(".claude/skills/state-review");
    fs::remove_file(&link).expect("remove global fixture link");
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/missing-target"), &link)
        .expect("create broken link");
    let broken = deployment
        .inspect(DeploymentQuery::for_target(global.clone()))
        .expect("inspect broken link")
        .items
        .into_iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("broken item");
    assert_eq!(broken.status, DeploymentStatus::Drift);
    assert_eq!(
        broken.observed.state,
        cc_switch_lib::ObservedDeploymentState::BrokenLink
    );

    fs::remove_file(&link).expect("remove broken link");
    std::os::unix::fs::symlink(home.join(".cc-switch/skills/state-review"), &link)
        .expect("recreate library link");
    fs::remove_dir_all(home.join(".cc-switch/skills/state-review")).expect("remove Library source");
    let library_missing = deployment
        .inspect(DeploymentQuery::for_target(global.clone()))
        .expect("inspect missing Library source")
        .items
        .into_iter()
        .find(|item| item.library_skill_id == skill.id)
        .expect("Library missing item");
    assert_eq!(library_missing.status, DeploymentStatus::Orphaned);
    assert_eq!(
        library_missing.observed.state,
        cc_switch_lib::ObservedDeploymentState::LibraryMissing
    );

    let unrecorded_source_root = tempfile::tempdir().expect("create unrecorded source");
    let unrecorded_source = unrecorded_source_root.path().join("unrecorded");
    write_skill(&unrecorded_source);
    fs::OpenOptions::new()
        .append(true)
        .open(unrecorded_source.join("SKILL.md"))
        .expect("open unrecorded manifest")
        .write_all(b"\nUnrecorded fixture.\n")
        .expect("differentiate unrecorded fixture");
    let unrecorded_skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &unrecorded_source,
        source(),
        None,
    )
    .expect("acquire unrecorded Library skill");
    let unrecorded_library = home.join(".cc-switch/skills/unrecorded");
    let unrecorded_link = home.join(".claude/skills/unrecorded");
    std::os::unix::fs::symlink(&unrecorded_library, &unrecorded_link)
        .expect("create unrecorded Library link");
    let unrecorded = deployment
        .inspect(DeploymentQuery::for_target(global.clone()))
        .expect("inspect unrecorded link")
        .items
        .into_iter()
        .find(|item| item.library_skill_id == unrecorded_skill.id)
        .expect("unrecorded item");
    assert_eq!(unrecorded.status, DeploymentStatus::Orphaned);
    assert_eq!(
        unrecorded.observed.state,
        cc_switch_lib::ObservedDeploymentState::UnrecordedLink
    );

    let archived_root = tempfile::tempdir().expect("create archived root");
    let archived_id = uuid::Uuid::new_v4().to_string();
    let archived_workspace = ProjectWorkspace {
        id: archived_id.clone(),
        display_name: "Archived project".to_string(),
        root_path: archived_root.path().to_path_buf(),
        root_kind: WorkspaceRootKind::NonGit,
        lifecycle: WorkspaceLifecycle::Archived,
        created_at: 1,
        updated_at: 1,
    };
    state
        .db
        .save_project_workspace(&archived_workspace)
        .expect("save archived workspace fixture");
    let archived_target = DeploymentTarget {
        consumer: DeploymentConsumer::Codex,
        workspace: WorkspaceKind::Project,
        workspace_id: archived_id,
    };
    let archived_desired = DesiredDeployment {
        id: uuid::Uuid::new_v4().to_string(),
        library_skill_id: skill.id,
        library_directory: "state-review".to_string(),
        target: archived_target.clone(),
        created_at: 1,
        updated_at: 1,
    };
    state
        .db
        .save_skill_deployment(&archived_desired)
        .expect("save archived desired fixture");
    let archived_link = archived_root.path().join(".agents/skills/state-review");
    fs::create_dir_all(archived_link.parent().expect("archived link parent"))
        .expect("create archived target root");
    let missing_source = home.join(".cc-switch/skills/state-review");
    fs::create_dir_all(&missing_source).expect("restore Library source");
    write_skill(&missing_source);
    std::os::unix::fs::symlink(&missing_source, &archived_link)
        .expect("create archived managed link");
    let archived = deployment
        .inspect(DeploymentQuery::for_target(archived_target))
        .expect("inspect archived target")
        .items
        .into_iter()
        .find(|item| item.library_directory == "state-review")
        .expect("archived item");
    assert_eq!(archived.status, DeploymentStatus::Archived);
    assert_eq!(
        archived.observed.state,
        cc_switch_lib::ObservedDeploymentState::CorrectLink
    );
}

#[allow(dead_code)]
fn _workspace_kind_is_shared() {
    assert_eq!(WorkspaceKind::Global, WorkspaceKind::Global);
}
