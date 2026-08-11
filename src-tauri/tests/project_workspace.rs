#![cfg(target_os = "macos")]

use std::fs;
use std::process::Command;

use cc_switch_lib::{
    DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome,
    DeploymentQuery, DeploymentStatus, DeploymentTarget, LibrarySkillAcquisitionService,
    LibrarySkillSource, LibrarySourceKind, ProjectWorkspaceService, SkillDeploymentService,
    WorkspaceKind, WorkspaceLifecycle, WorkspaceRootKind, WorkspaceScopeKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn registers_a_git_repository_at_its_canonical_root_without_creating_consumer_directories() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let repository = tempfile::tempdir().expect("create repository");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repository.path())
        .status()
        .expect("run git init");
    assert!(status.success());
    let nested = repository.path().join("src");
    std::fs::create_dir_all(&nested).expect("create nested directory");

    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let registration = service
        .register(&nested, None)
        .expect("register repository");

    assert_eq!(
        registration.workspace.root_path,
        std::fs::canonicalize(repository.path()).expect("canonical repository")
    );
    assert!(!repository.path().join(".claude").exists());
    assert!(!repository.path().join(".agents").exists());
}

#[test]
fn registers_a_non_git_directory_at_the_selected_physical_root() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let selected = tempfile::tempdir().expect("create non-git root");
    let nested = selected.path().join("nested");
    std::fs::create_dir_all(&nested).expect("create nested directory");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());

    let registration = service
        .register(&nested, None)
        .expect("register non-git root");

    assert_eq!(registration.workspace.root_kind, WorkspaceRootKind::NonGit);
    assert_eq!(
        registration.workspace.root_path,
        std::fs::canonicalize(&nested).expect("canonical selected root")
    );
}

#[test]
fn registers_a_linked_worktree_at_the_worktree_root() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    run_git(repository.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repository.path().join("README.md"), "root\n").expect("write commit");
    run_git(repository.path(), &["add", "README.md"]);
    run_git(repository.path(), &["commit", "--quiet", "-m", "initial"]);
    let worktree = repository.path().join("worktree");
    run_git(
        repository.path(),
        &["worktree", "add", "--quiet", "-b", "feature", "worktree"],
    );
    let nested = worktree.join("src");
    std::fs::create_dir_all(&nested).expect("create nested worktree directory");

    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let registration = service.register(&nested, None).expect("register worktree");

    assert_eq!(
        registration.workspace.root_kind,
        WorkspaceRootKind::GitWorktree
    );
    assert_eq!(
        registration.workspace.root_path,
        std::fs::canonicalize(&worktree).expect("canonical worktree root")
    );
}

#[test]
fn rejects_canonical_ancestor_and_descendant_workspace_overlap() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let root = tempfile::tempdir().expect("create root");
    let child = root.path().join("child");
    std::fs::create_dir_all(&child).expect("create child");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());

    service.register(root.path(), None).expect("register root");
    let error = service
        .register(&child, None)
        .expect_err("descendant must overlap");
    assert!(error.to_string().contains("overlaps"));
}

#[test]
fn scans_existing_root_and_nested_consumer_scopes_without_writing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let root = tempfile::tempdir().expect("create root");
    let claude = root.path().join(".claude/skills/root-skill");
    let nested = root.path().join("packages/one/.agents/skills/nested-skill");
    std::fs::create_dir_all(&claude).expect("create root-level skill");
    std::fs::create_dir_all(&nested).expect("create nested skill");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let canonical_root = std::fs::canonicalize(root.path()).expect("canonical root");

    let before = std::fs::read_dir(root.path())
        .expect("read root before")
        .count();
    let scan = service.inspect_path(root.path()).expect("scan root");
    let after = std::fs::read_dir(root.path())
        .expect("read root after")
        .count();

    assert_eq!(before, after);
    assert!(scan.scopes.iter().any(|scope| {
        scope.path == canonical_root.join(".claude/skills")
            && scope.kind == WorkspaceScopeKind::RootLevel
            && scope.skill_directories == vec!["root-skill".to_string()]
    }));
    assert!(scan.scopes.iter().any(|scope| {
        scope.path == canonical_root.join("packages/one/.agents/skills")
            && scope.kind == WorkspaceScopeKind::NestedUnsupported
    }));
}

#[test]
fn project_deployment_lazily_creates_absolute_links_and_precise_git_excludes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("review-skill");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        LibrarySkillSource {
            kind: LibrarySourceKind::Zip,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        None,
    )
    .expect("acquire skill");
    let workspace_service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = workspace_service
        .register(repository.path(), None)
        .expect("register repository")
        .workspace;
    let deployment = SkillDeploymentService::new(state.db.clone());
    let target = DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id.clone(),
    };

    let before = deployment
        .inspect(DeploymentQuery::for_target(target.clone()))
        .expect("inspect before deployment");
    assert_eq!(before.items[0].status, DeploymentStatus::NotDeployed);
    assert!(!repository.path().join(".claude").exists());

    let applied = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy project skill");
    assert_eq!(applied.items[0].outcome, DeploymentMutationOutcome::Applied);
    let link = repository.path().join(".claude/skills/review-skill");
    assert_eq!(
        fs::read_link(&link).expect("read project link"),
        ensure_test_home().join(".cc-switch/skills/review-skill")
    );
    assert!(
        fs::symlink_metadata(repository.path().join(".claude/skills"))
            .expect("inspect project target root")
            .is_dir()
    );
    let exclude = repository.path().join(".git/info/exclude");
    let marker = "/.claude/skills/review-skill # cc-switch managed";
    let content = fs::read_to_string(&exclude).expect("read local Git exclude");
    assert_eq!(content.lines().filter(|line| *line == marker).count(), 1);
    assert!(!repository.path().join(".gitignore").exists());

    let repeated = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("repeat project deploy");
    assert_eq!(
        repeated.items[0].outcome,
        DeploymentMutationOutcome::AlreadyInSync
    );
    let content = fs::read_to_string(&exclude).expect("read repeated Git exclude");
    assert_eq!(content.lines().filter(|line| *line == marker).count(), 1);

    let removed = deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Undeploy {
            library_skill_id: skill.id,
            target,
        }))
        .expect("undeploy project skill");
    assert_eq!(removed.items[0].outcome, DeploymentMutationOutcome::Removed);
    assert!(!link.exists());
    let content = fs::read_to_string(exclude).expect("read undeployed Git exclude");
    assert!(!content.lines().any(|line| line == marker));
}

#[test]
fn archive_hides_workspace_without_touching_project_deployment_or_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let repository = tempfile::tempdir().expect("create repository");
    run_git(repository.path(), &["init", "--quiet"]);
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("archive-skill");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        LibrarySkillSource {
            kind: LibrarySourceKind::Zip,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        None,
    )
    .expect("acquire skill");
    let workspace_service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = workspace_service
        .register(repository.path(), None)
        .expect("register repository")
        .workspace;
    let target = cc_switch_lib::DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id.clone(),
    };
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id,
            target,
        }))
        .expect("deploy archive fixture");

    let readme = repository.path().join("README.md");
    fs::write(&readme, "must remain\n").expect("write project content");
    let readme_before = fs::read(&readme).expect("read project content");
    let exclude = repository.path().join(".git/info/exclude");
    let exclude_before = fs::read(&exclude).expect("read local exclude");
    let link = repository.path().join(".claude/skills/archive-skill");
    let link_before = fs::read_link(&link).expect("read deployed link");

    let archived = workspace_service
        .archive(&workspace.id)
        .expect("archive workspace");
    assert_eq!(archived.lifecycle, WorkspaceLifecycle::Archived);
    assert!(workspace_service
        .list(false)
        .expect("list active workspaces")
        .is_empty());
    assert_eq!(
        workspace_service
            .list(true)
            .expect("list all workspaces")
            .len(),
        1
    );
    assert_eq!(
        fs::read(&readme).expect("read project content"),
        readme_before
    );
    assert_eq!(
        fs::read(&exclude).expect("read local exclude"),
        exclude_before
    );
    assert_eq!(
        fs::read_link(&link).expect("read deployed link"),
        link_before
    );
}

#[test]
fn lifecycle_transitions_persist_unavailable_and_reject_invalid_operations() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let root = tempfile::tempdir().expect("create workspace root");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let registered = service
        .register(root.path(), None)
        .expect("register workspace");
    let id = registered.workspace.id.clone();

    let renamed = service
        .rename(&id, "Renamed workspace".to_string())
        .expect("rename workspace");
    assert_eq!(renamed.display_name, "Renamed workspace");
    let archived = service.archive(&id).expect("archive workspace");
    assert_eq!(archived.lifecycle, WorkspaceLifecycle::Archived);
    assert!(service.archive(&id).is_err(), "archiving twice must fail");
    assert!(service.list(false).expect("list active").is_empty());
    assert_eq!(service.list(true).expect("list archived").len(), 1);

    let restored = service.restore(&id).expect("restore workspace");
    assert_eq!(restored.lifecycle, WorkspaceLifecycle::Active);
    assert!(service.restore(&id).is_err(), "restoring active must fail");

    drop(root);
    let unavailable = service
        .list(false)
        .expect("refresh unavailable workspace")
        .into_iter()
        .find(|workspace| workspace.id == id)
        .expect("unavailable workspace remains visible");
    assert_eq!(unavailable.lifecycle, WorkspaceLifecycle::Unavailable);
    assert_eq!(
        state
            .db
            .get_project_workspace(&id)
            .expect("read persisted lifecycle")
            .expect("workspace row")
            .lifecycle,
        WorkspaceLifecycle::Unavailable
    );
    assert!(
        service.restore(&id).is_err(),
        "unavailable cannot restore directly"
    );
    let archived = service.archive(&id).expect("archive unavailable workspace");
    assert_eq!(archived.lifecycle, WorkspaceLifecycle::Archived);
    let restored_unavailable = service.restore(&id).expect("restore unavailable root");
    assert_eq!(
        restored_unavailable.lifecycle,
        WorkspaceLifecycle::Unavailable
    );
    assert!(service.forget(&id).is_err(), "forget unavailable must fail");
    let archived = service.archive(&id).expect("archive before forget");
    assert_eq!(archived.lifecycle, WorkspaceLifecycle::Archived);
    assert!(service.forget(&id).expect("forget archived workspace"));
    assert!(state
        .db
        .get_project_workspace(&id)
        .expect("read forgotten workspace")
        .is_none());
}

#[test]
fn active_workspace_retargeted_through_a_symlink_becomes_persisted_unavailable() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let original = tempfile::tempdir().expect("create original root");
    let original_path = original.path().to_path_buf();
    let foreign = tempfile::tempdir().expect("create foreign root");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&original_path, None)
        .expect("register root")
        .workspace;
    let parked = tempfile::tempdir().expect("create parked parent");
    let parked_path = parked.path().join("original");
    fs::rename(&original_path, &parked_path).expect("move original root");
    std::os::unix::fs::symlink(foreign.path(), &original_path).expect("retarget root symlink");

    let refreshed = service
        .list(false)
        .expect("refresh retargeted root")
        .into_iter()
        .find(|item| item.id == workspace.id)
        .expect("workspace row");
    assert_eq!(refreshed.lifecycle, WorkspaceLifecycle::Unavailable);
    assert_eq!(
        state
            .db
            .get_project_workspace(&workspace.id)
            .expect("read persisted workspace")
            .expect("workspace row")
            .lifecycle,
        WorkspaceLifecycle::Unavailable
    );
}

#[test]
fn relocation_accepts_moved_git_history_after_new_commits() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create repository parent");
    let original = parent.path().join("original");
    fs::create_dir_all(&original).expect("create original repository");
    init_git_with_commit(&original, "first\n");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&original, None)
        .expect("register repository")
        .workspace;
    fs::write(original.join("second.txt"), "second\n").expect("write second commit");
    run_git(&original, &["add", "second.txt"]);
    run_git(&original, &["commit", "--quiet", "-m", "second"]);
    let moved = parent.path().join("moved");
    fs::rename(&original, &moved).expect("move repository root");

    let relocated = service
        .relocate(&workspace.id, &moved)
        .expect("relocate repository after new commit");
    assert_eq!(
        relocated.outcome,
        cc_switch_lib::WorkspaceRelocationOutcome::Relocated
    );
    assert_eq!(relocated.workspace.id, workspace.id);
    assert_eq!(
        relocated.workspace.root_path,
        fs::canonicalize(&moved).expect("canonical moved root")
    );
    assert_eq!(relocated.workspace.lifecycle, WorkspaceLifecycle::Active);
}

#[test]
fn rejects_cloned_repository_as_relocation_but_allows_distinct_registration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create repository parent");
    let seed = parent.path().join("seed");
    fs::create_dir_all(&seed).expect("create seed repository");
    init_git_with_commit(&seed, "shared root\n");

    let first = parent.path().join("first-clone");
    let second = parent.path().join("second-clone");
    let seed_arg = seed.to_string_lossy().to_string();
    let first_arg = first.to_string_lossy().to_string();
    let second_arg = second.to_string_lossy().to_string();
    run_git(
        parent.path(),
        &["clone", "--quiet", seed_arg.as_str(), first_arg.as_str()],
    );
    run_git(
        parent.path(),
        &["clone", "--quiet", seed_arg.as_str(), second_arg.as_str()],
    );
    run_git(
        &second,
        &["config", "user.email", "workspace-tests@example.com"],
    );
    run_git(&second, &["config", "user.name", "Workspace Tests"]);
    fs::write(second.join("README.md"), "divergent history\n").expect("write divergent commit");
    run_git(&second, &["add", "README.md"]);
    run_git(&second, &["commit", "--quiet", "-m", "divergent"]);

    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let first_workspace = service
        .register(&first, None)
        .expect("register first clone")
        .workspace;
    let first_gone = parent.path().join("first-gone");
    fs::rename(&first, &first_gone).expect("make first clone unavailable");
    let error = service
        .relocate(&first_workspace.id, &second)
        .expect_err("a clone sharing the root commit must not replace the original identity");
    assert!(
        error.to_string().contains("already registered")
            || error.to_string().contains("Relocate")
            || error.to_string().contains("identity")
    );
    service
        .register(&second, None)
        .expect("a separately cloned repository remains a distinct registration");
}

#[test]
fn explicit_register_of_a_moved_repository_rejects_duplicate_identity() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create repository parent");
    let original = parent.path().join("original");
    fs::create_dir_all(&original).expect("create original repository");
    init_git_with_commit(&original, "move me\n");
    let moved = parent.path().join("moved");

    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    service
        .register(&original, None)
        .expect("register original repository");
    fs::rename(&original, &moved).expect("move repository root");

    let error = service
        .register(&moved, None)
        .expect_err("explicit registration of a moved root must not duplicate the row");
    assert!(
        error.to_string().contains("already registered")
            || error.to_string().contains("Relocate")
            || error.to_string().contains("identity")
    );
}

#[test]
fn relocation_uses_worktree_admin_identity_and_rejects_other_repositories() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create repository parent");
    let repository = parent.path().join("repository");
    fs::create_dir_all(&repository).expect("create repository");
    init_git_with_commit(&repository, "root\n");
    let first = repository.join("first-worktree");
    run_git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "first",
            "first-worktree",
        ],
    );
    run_git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "second",
            "second-worktree",
        ],
    );
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&first, None)
        .expect("register first worktree")
        .workspace;
    let moved = repository.join("moved-first");
    run_git(
        &repository,
        &["worktree", "move", "first-worktree", "moved-first"],
    );
    let relocated = service
        .relocate(&workspace.id, &moved)
        .expect("relocate linked worktree");
    assert_eq!(
        relocated.outcome,
        cc_switch_lib::WorkspaceRelocationOutcome::Relocated
    );
    assert_eq!(relocated.workspace.lifecycle, WorkspaceLifecycle::Active);

    let unavailable_root = parent.path().join("unavailable");
    let wrong_candidate = parent.path().join("wrong");
    fs::create_dir_all(&unavailable_root).expect("create unavailable root");
    init_git_with_commit(&unavailable_root, "unavailable\n");
    let wrong_workspace = service
        .register(&unavailable_root, None)
        .expect("register second repository")
        .workspace;
    fs::rename(&unavailable_root, parent.path().join("gone")).expect("remove old repository root");
    fs::create_dir_all(&wrong_candidate).expect("create wrong candidate");
    init_git_with_commit(&wrong_candidate, "wrong\n");
    assert!(service
        .relocate(&wrong_workspace.id, &wrong_candidate)
        .is_err());

    let empty_old = parent.path().join("empty-old");
    let empty_moved = parent.path().join("empty-moved");
    fs::create_dir_all(&empty_old).expect("create empty repository");
    run_git(&empty_old, &["init", "--quiet"]);
    let empty_workspace = service
        .register(&empty_old, None)
        .expect("register empty repository")
        .workspace;
    fs::rename(&empty_old, &empty_moved).expect("move empty repository root");
    let relocated_empty = service
        .relocate(&empty_workspace.id, &empty_moved)
        .expect("relocate empty repository by filesystem identity");
    assert_eq!(
        relocated_empty.outcome,
        cc_switch_lib::WorkspaceRelocationOutcome::Relocated
    );

    let empty_gone = parent.path().join("empty-gone");
    fs::rename(&empty_moved, &empty_gone).expect("remove empty repository root");
    let empty_candidate = parent.path().join("empty-candidate");
    fs::create_dir_all(&empty_candidate).expect("create empty candidate");
    run_git(&empty_candidate, &["init", "--quiet"]);
    assert!(service
        .relocate(&empty_workspace.id, &empty_candidate)
        .is_err());
}

#[test]
fn relocation_rejects_a_reappeared_old_root_and_restore_keeps_foreign_git_root_unavailable() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create repository parent");
    let original = parent.path().join("original");
    fs::create_dir_all(&original).expect("create original repository");
    init_git_with_commit(&original, "original\n");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&original, None)
        .expect("register repository")
        .workspace;
    let gone = parent.path().join("gone");
    fs::rename(&original, &gone).expect("remove original path");
    assert_eq!(
        service
            .list(false)
            .expect("persist unavailable")
            .into_iter()
            .find(|item| item.id == workspace.id)
            .expect("workspace row")
            .lifecycle,
        WorkspaceLifecycle::Unavailable
    );
    fs::create_dir_all(&original).expect("recreate old path");
    init_git_with_commit(&original, "foreign\n");
    let distinct = parent.path().join("distinct");
    fs::create_dir_all(&distinct).expect("create distinct candidate");
    let relocation = service
        .relocate(&workspace.id, &distinct)
        .expect("register distinct candidate when old root reappeared");
    assert_eq!(
        relocation.outcome,
        cc_switch_lib::WorkspaceRelocationOutcome::RegisteredDistinct
    );
    assert_ne!(relocation.workspace.id, workspace.id);
    assert_eq!(
        service
            .get(&workspace.id)
            .expect("read original workspace")
            .expect("original row")
            .lifecycle,
        WorkspaceLifecycle::Unavailable
    );

    let archived = service.archive(&workspace.id).expect("archive unavailable");
    assert_eq!(archived.lifecycle, WorkspaceLifecycle::Archived);
    let restored = service
        .restore(&workspace.id)
        .expect("restore foreign root");
    assert_eq!(restored.lifecycle, WorkspaceLifecycle::Unavailable);
    assert!(
        service
            .get(&workspace.id)
            .expect("read restored workspace")
            .expect("workspace row")
            .lifecycle
            == WorkspaceLifecycle::Unavailable
    );
}

#[test]
fn non_git_relocation_rejects_unrelated_root_but_allows_distinct_registration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let old = tempfile::tempdir().expect("create old non-git root");
    let candidate = tempfile::tempdir().expect("create candidate non-git root");
    let old_path = old.path().to_path_buf();
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&old_path, None)
        .expect("register old root")
        .workspace;
    drop(old);
    assert!(service.relocate(&workspace.id, candidate.path()).is_err());
    let distinct = service
        .register(candidate.path(), None)
        .expect("explicitly register unrelated non-git candidate")
        .workspace;
    assert_ne!(distinct.id, workspace.id);
    assert_eq!(
        service
            .get(&workspace.id)
            .expect("read original workspace")
            .expect("original row")
            .lifecycle,
        WorkspaceLifecycle::Unavailable
    );
}

#[test]
fn non_git_rename_relocation_preserves_workspace_identity() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let parent = tempfile::tempdir().expect("create non-git parent");
    let original = parent.path().join("original");
    let moved = parent.path().join("moved");
    fs::create_dir_all(&original).expect("create original non-git root");
    let state = create_test_state().expect("create test state");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(&original, None)
        .expect("register non-git root")
        .workspace;
    fs::rename(&original, &moved).expect("move non-git root");

    let relocated = service
        .relocate(&workspace.id, &moved)
        .expect("relocate same non-git filesystem object");
    assert_eq!(
        relocated.outcome,
        cc_switch_lib::WorkspaceRelocationOutcome::Relocated
    );
    assert_eq!(relocated.workspace.id, workspace.id);
    assert_eq!(relocated.workspace.lifecycle, WorkspaceLifecycle::Active);
}

#[test]
fn forget_requires_zero_project_deployments_and_never_deletes_project_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let repository = tempfile::tempdir().expect("create repository");
    init_git_with_commit(repository.path(), "forget fixture\n");
    let source_root = tempfile::tempdir().expect("create source root");
    let source_dir = source_root.path().join("forget-skill");
    write_skill(&source_dir);
    let state = create_test_state().expect("create test state");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source_dir,
        LibrarySkillSource {
            kind: LibrarySourceKind::Zip,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        None,
    )
    .expect("acquire skill");
    let service = ProjectWorkspaceService::new(state.db.clone());
    let workspace = service
        .register(repository.path(), None)
        .expect("register repository")
        .workspace;
    let target = cc_switch_lib::DeploymentTarget {
        consumer: DeploymentConsumer::Claude,
        workspace: WorkspaceKind::Project,
        workspace_id: workspace.id.clone(),
    };
    let deployment = SkillDeploymentService::new(state.db.clone());
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: skill.id.clone(),
            target: target.clone(),
        }))
        .expect("deploy before archive");
    let link = repository.path().join(".claude/skills/forget-skill");
    let content_before = fs::read(repository.path().join("README.md")).unwrap_or_default();
    service.archive(&workspace.id).expect("archive workspace");
    assert!(service.forget(&workspace.id).is_err());
    assert!(link.exists(), "forget guard must retain project link");

    service.restore(&workspace.id).expect("restore workspace");
    deployment
        .apply(DeploymentBatch::single(DeploymentIntent::Forget {
            library_skill_id: skill.id,
            target,
        }))
        .expect("forget deployment accounting");
    service
        .archive(&workspace.id)
        .expect("archive after accounting");
    assert!(service.forget(&workspace.id).expect("forget workspace"));
    assert!(
        link.exists(),
        "workspace forget must not remove project link"
    );
    assert_eq!(
        fs::read(repository.path().join("README.md")).expect("read project content"),
        content_before
    );
}

fn write_skill(dir: &std::path::Path) {
    fs::create_dir_all(dir).expect("create skill tree");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: deploy-review\ndescription: Deploy review workflow\n---\n\nReview carefully.\n",
    )
    .expect("write manifest");
}

fn run_git(directory: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(directory)
        .status()
        .expect("run git");
    assert!(status.success(), "git command failed: git {:?}", args);
}

fn init_git_with_commit(directory: &std::path::Path, content: &str) {
    run_git(directory, &["init", "--quiet"]);
    run_git(
        directory,
        &["config", "user.email", "workspace-tests@example.com"],
    );
    run_git(directory, &["config", "user.name", "Workspace Tests"]);
    fs::write(directory.join("README.md"), content).expect("write repository content");
    run_git(directory, &["add", "README.md"]);
    run_git(directory, &["commit", "--quiet", "-m", "initial"]);
}
