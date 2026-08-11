#![cfg(target_os = "macos")]

use std::fs;

use cc_switch_lib::{
    DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome,
    DeploymentQuery, DeploymentStatus, DeploymentTarget, LibrarySkillAcquisitionService,
    LibrarySkillSource, LibrarySourceKind, SkillDeploymentService, WorkspaceKind,
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

#[allow(dead_code)]
fn _workspace_kind_is_shared() {
    assert_eq!(WorkspaceKind::Global, WorkspaceKind::Global);
}
