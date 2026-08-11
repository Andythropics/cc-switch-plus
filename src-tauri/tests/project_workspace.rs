#![cfg(target_os = "macos")]

use std::fs;
use std::process::Command;

use cc_switch_lib::{
    DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome,
    DeploymentQuery, DeploymentStatus, DeploymentTarget, LibrarySkillAcquisitionService,
    LibrarySkillSource, LibrarySourceKind, ProjectWorkspaceService, SkillDeploymentService,
    WorkspaceKind, WorkspaceRootKind, WorkspaceScopeKind,
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
