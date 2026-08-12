#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use cc_switch_lib::{
    ActivityDetailCode, ActivityOperation, ActivityOutcome, ActivityQuery, ActivityReason,
    ActivityRecorder, ProjectSkillImportScope, ProjectSkillImportService,
    ProjectSkillImportValidationStatus, WorkspaceLifecycle,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

#[test]
fn inspect_reports_a_root_level_unmanaged_skill_without_writing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    let source = project.path().join(".claude/skills/import-me");
    fs::create_dir_all(&source).expect("create unmanaged skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: import-me\ndescription: Imported skill\nunknown: preserve-me\n---\n\nBody.\n",
    )
    .expect("write skill manifest");
    fs::write(source.join("notes.txt"), "unknown supporting bytes\n")
        .expect("write supporting file");

    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), Some("Import fixture".to_string()))
        .expect("register project")
        .workspace;
    assert_eq!(workspace.lifecycle, WorkspaceLifecycle::Active);
    let service = ProjectSkillImportService::new(state.db.clone());
    let before_entries = fs::read_dir(project.path())
        .expect("read project before inspect")
        .count();

    let inspection = service
        .inspect(&workspace.id)
        .expect("inspect project imports");
    assert_eq!(inspection.workspace_id, workspace.id);
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "import-me")
        .expect("root-level finding");
    assert_eq!(finding.scope, ProjectSkillImportScope::RootLevel);
    assert_eq!(
        finding.validation.status,
        ProjectSkillImportValidationStatus::Valid
    );
    assert!(finding.compatibility.claude.compatible);
    assert!(finding.compatibility.codex.compatible);
    assert!(!finding.git.tracked);
    assert!(matches!(
        finding.library_match,
        cc_switch_lib::ProjectSkillImportLibraryMatch::None
    ));
    assert!(finding.replace_eligibility.eligible);
    assert_eq!(
        state.db.list_library_skills().expect("list Library"),
        vec![]
    );
    assert_eq!(
        fs::read_dir(project.path())
            .expect("read project after inspect")
            .count(),
        before_entries,
        "inspection must not create project roots"
    );
}

#[test]
fn import_only_admits_a_local_snapshot_without_deploying_project_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    let source = project.path().join(".claude/skills/local-snapshot");
    fs::create_dir_all(&source).expect("create unmanaged skill");
    let manifest = "---\nname: local-snapshot\ndescription: Local snapshot\nunknown: preserve-me\n---\n\nBody bytes.\n";
    fs::write(source.join("SKILL.md"), manifest).expect("write manifest");
    fs::write(source.join("notes.bin"), [0_u8, 1, 2, 255]).expect("write unknown bytes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(source.join("notes.bin"), fs::Permissions::from_mode(0o751))
            .expect("set source mode");
    }

    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "local-snapshot")
        .expect("finding");
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id.clone(),
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token.clone(),
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "local-snapshot-import".to_string(),
                display_name: None,
            },
        })
        .expect("apply local import");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Created
    );
    let library = state
        .db
        .get_library_skill_by_id(result.library_skill_id.as_deref().expect("library id"))
        .expect("read library")
        .expect("library row");
    assert_eq!(
        library.source.kind,
        cc_switch_lib::LibrarySourceKind::LocalImport
    );
    assert!(library.source.url.is_none());
    assert!(library.source.repo_owner.is_none());
    assert_eq!(
        fs::read_to_string(source.join("SKILL.md")).expect("read source"),
        manifest
    );
    assert_eq!(
        fs::read(source.join("notes.bin")).expect("read unknown bytes"),
        vec![0, 1, 2, 255]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::var("CC_SWITCH_TEST_HOME").expect("test home");
        let copied = Path::new(&home).join(".cc-switch/skills/local-snapshot-import/notes.bin");
        assert_eq!(
            fs::metadata(copied)
                .expect("read copied mode")
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
    }
    assert!(
        !source.is_symlink(),
        "import-only must leave project source real"
    );
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Import),
            ..ActivityQuery::default()
        })
        .expect("list import activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Success);
    assert_eq!(
        activity.entries[0].target.library_skill_id,
        result.library_skill_id
    );
}

#[test]
fn import_and_replace_backs_up_source_and_deploys_expected_link() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    let source = project.path().join(".claude/skills/replace-me");
    fs::create_dir_all(&source).expect("create unmanaged skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: replace-me\ndescription: Replace me\n---\n\nBody.\n",
    )
    .expect("write manifest");
    fs::write(source.join("unknown.txt"), "preserve me").expect("write content");
    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "replace-me")
        .expect("finding");
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id.clone(),
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "replace-me".to_string(),
                display_name: None,
            },
        })
        .expect("apply and replace");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Deployed
    );
    assert!(
        source.is_symlink(),
        "source should become a Deployment link"
    );
    let target = fs::read_link(&source).expect("read deployment link");
    assert!(target.is_absolute());
    assert!(target.ends_with(".cc-switch/skills/replace-me"));
    let backup = result.backup_path.expect("backup path");
    assert!(Path::new(&backup).is_dir());
    assert_eq!(
        fs::read_to_string(Path::new(&backup).join("source/SKILL.md")).expect("read backup"),
        "---\nname: replace-me\ndescription: Replace me\n---\n\nBody.\n"
    );
    let desired = state.db.list_skill_deployments().expect("list desired");
    assert_eq!(desired.len(), 1);
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            reason: Some(ActivityReason::ImportAndReplace),
            ..ActivityQuery::default()
        })
        .expect("list import-and-replace activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Success);
    assert_eq!(activity.entries[0].operation, ActivityOperation::Library);
    let low_level = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Deployment),
            ..ActivityQuery::default()
        })
        .expect("list nested deployment activity");
    assert_eq!(low_level.entries.len(), 1);
    assert_eq!(low_level.entries[0].reason, ActivityReason::Deploy);
    assert_eq!(low_level.entries[0].outcome, ActivityOutcome::Success);
    assert_eq!(
        low_level.entries[0].target.library_skill_id,
        result.library_skill_id
    );
    assert_eq!(
        low_level.entries[0].target.workspace_id.as_deref(),
        Some(workspace.id.as_str())
    );
}

#[test]
fn tracked_source_blocks_import_and_replace_without_touching_index_or_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(project.path())
        .status()
        .expect("git init");
    let source = project.path().join(".claude/skills/tracked");
    fs::create_dir_all(&source).expect("create tracked skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: tracked\ndescription: Tracked\n---\n\nBody.\n",
    )
    .expect("write manifest");
    Command::new("git")
        .args(["add", "."])
        .current_dir(project.path())
        .status()
        .expect("git add");
    Command::new("git")
        .args([
            "-c",
            "user.email=cc-switch@example.invalid",
            "-c",
            "user.name=cc-switch",
            "commit",
            "--quiet",
            "-m",
            "tracked fixture",
        ])
        .current_dir(project.path())
        .status()
        .expect("git commit");
    let before_status = String::from_utf8(
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(project.path())
            .output()
            .expect("git status")
            .stdout,
    )
    .expect("status utf8");

    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "tracked")
        .expect("finding");
    assert!(finding.git.tracked);
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "tracked".to_string(),
                display_name: None,
            },
        })
        .expect("structured tracked blocker");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Blocked
    );
    assert_eq!(
        result.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::GitTrackedContent)
    );
    assert!(source.is_dir());
    assert_eq!(
        String::from_utf8(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(project.path())
                .output()
                .expect("git status after")
                .stdout,
        )
        .expect("status utf8 after"),
        before_status
    );
}

#[test]
fn changed_source_returns_stale_without_creating_a_library_row() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    let source = project.path().join(".claude/skills/stale");
    fs::create_dir_all(&source).expect("create skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: stale\ndescription: Stale\n---\n\nOriginal.\n",
    )
    .expect("write manifest");
    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "stale")
        .expect("finding")
        .clone();
    fs::write(source.join("notes.txt"), "changed after preview").expect("change source");
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id,
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "stale".to_string(),
                display_name: None,
            },
        })
        .expect("structured stale result");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Stale
    );
    assert!(state
        .db
        .list_library_skills()
        .expect("list library")
        .is_empty());
    assert!(source.is_dir());
}

#[test]
fn deployment_failure_rolls_back_source_library_and_desired_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("create project root");
    let source = project.path().join(".claude/skills/failure");
    fs::create_dir_all(&source).expect("create skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: failure\ndescription: Failure\n---\n\nBody.\n",
    )
    .expect("write manifest");
    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "failure")
        .expect("finding")
        .clone();
    state
        .db
        .fail_skill_deployment_inserts_for_test()
        .expect("install deployment failure trigger");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(true);
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id,
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "failure".to_string(),
                display_name: None,
            },
        })
        .expect("structured rollback result");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(false);
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::RolledBack
    );
    assert!(result
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("injected deployment insert failure"));
    assert!(source.is_dir());
    assert!(!source.is_symlink());
    assert!(state
        .db
        .list_library_skills()
        .expect("list library")
        .is_empty());
    assert!(state
        .db
        .list_skill_deployments()
        .expect("list desired")
        .is_empty());
    assert!(result
        .backup_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_dir()));
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            reason: Some(ActivityReason::ImportAndReplace),
            ..ActivityQuery::default()
        })
        .expect("list rolled-back import activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::RolledBack);
    assert_eq!(
        activity.entries[0].detail_code,
        ActivityDetailCode::FilesystemFailure
    );
}

#[test]
fn deployment_compensation_failure_cleans_expected_link_before_restoring_source() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join(".claude/skills/compensation");
    fs::create_dir_all(&source).expect("create Skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: compensation\ndescription: Compensation\n---\n\nBody.\n",
    )
    .expect("write manifest");
    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "compensation")
        .expect("finding");
    state
        .db
        .fail_skill_deployment_inserts_for_test()
        .expect("inject deployment DB failure");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(true);
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "compensation".to_string(),
                display_name: None,
            },
        })
        .expect("structured compensation result");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(false);
    assert!(matches!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::RolledBack
            | cc_switch_lib::ProjectSkillImportOutcome::RecoveryRequired
    ));
    assert!(result
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("injected compensation failure"));
    assert!(
        source.is_dir(),
        "source must be restored as a real directory"
    );
    assert!(
        !source.is_symlink(),
        "expected Deployment link must be removed"
    );
    assert!(state
        .db
        .list_skill_deployments()
        .expect("list desired")
        .is_empty());
}

#[test]
fn identical_content_reuses_existing_library_identity() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let library_source = tempfile::tempdir().expect("library source");
    fs::write(
        library_source.path().join("SKILL.md"),
        "---\nname: reuse\ndescription: Reuse\n---\n\nSame bytes.\n",
    )
    .expect("write library manifest");
    let state = create_test_state().expect("create test state");
    let existing = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        library_source.path(),
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::Git,
            url: Some("https://example.invalid/reuse".to_string()),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("existing-reuse"),
    )
    .expect("acquire existing library");
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join(".claude/skills/reuse");
    fs::create_dir_all(&source).expect("create project skill");
    fs::copy(
        library_source.path().join("SKILL.md"),
        source.join("SKILL.md"),
    )
    .expect("copy identical manifest");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "reuse")
        .expect("finding");
    match &finding.library_match {
        cc_switch_lib::ProjectSkillImportLibraryMatch::Identical {
            library_skill_id, ..
        } => assert_eq!(library_skill_id, &existing.id),
        other => panic!("expected identical Library match, got {other:?}"),
    }
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::Reuse {
                library_skill_id: existing.id.clone(),
            },
        })
        .expect("reuse identical Library");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Reused
    );
    assert_eq!(
        result.library_skill_id.as_deref(),
        Some(existing.id.as_str())
    );
    assert_eq!(
        state.db.list_library_skills().expect("list Library").len(),
        1
    );
}

#[test]
fn nested_scope_is_reported_but_cannot_be_imported() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join("packages/app/.agents/skills/nested");
    fs::create_dir_all(&source).expect("create nested skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: nested\ndescription: Nested\n---\n\nBody.\n",
    )
    .expect("write nested manifest");
    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "nested")
        .expect("nested finding");
    assert_eq!(
        finding.scope,
        cc_switch_lib::ProjectSkillImportScope::NestedUnsupported
    );
    assert_eq!(
        finding.replace_eligibility.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::NestedUnsupported)
    );
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "nested".to_string(),
                display_name: None,
            },
        })
        .expect("structured nested blocker");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Blocked
    );
    assert_eq!(
        result.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::NestedUnsupported)
    );
    assert!(state
        .db
        .list_library_skills()
        .expect("list Library")
        .is_empty());
}

#[test]
fn import_and_replace_blocks_when_identical_library_uses_another_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let library_source = tempfile::tempdir().expect("library source");
    fs::write(
        library_source.path().join("SKILL.md"),
        "---\nname: alias\ndescription: Alias\n---\n\nAlias bytes.\n",
    )
    .expect("write library manifest");
    let state = create_test_state().expect("create test state");
    let existing = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        library_source.path(),
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::Git,
            url: Some("https://example.invalid/alias".to_string()),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("library-alias"),
    )
    .expect("acquire alias Library");
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join(".claude/skills/project-name");
    fs::create_dir_all(&source).expect("create project skill");
    fs::copy(
        library_source.path().join("SKILL.md"),
        source.join("SKILL.md"),
    )
    .expect("copy source");
    let state_workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service
        .inspect(&state_workspace.id)
        .expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "project-name")
        .expect("finding");
    assert!(matches!(
        finding.library_match,
        cc_switch_lib::ProjectSkillImportLibraryMatch::Identical { .. }
    ));
    assert_eq!(
        finding.replace_eligibility.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::DirectoryIdentityMismatch)
    );
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: state_workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::Reuse {
                library_skill_id: existing.id,
            },
        })
        .expect("structured identity blocker");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Blocked
    );
    assert_eq!(
        result.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::DirectoryIdentityMismatch)
    );
    assert!(source.is_dir());
}

#[test]
fn confirmed_replace_library_updates_snapshot_in_place_without_changing_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let old_source = tempfile::tempdir().expect("old Library source");
    fs::write(
        old_source.path().join("SKILL.md"),
        "---\nname: replace-library\ndescription: Old\n---\n\nOld bytes.\n",
    )
    .expect("write old manifest");
    let state = create_test_state().expect("create test state");
    let existing = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        old_source.path(),
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::Git,
            url: Some("https://example.invalid/old".to_string()),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("replace-library"),
    )
    .expect("acquire old Library");
    let old_hash = existing.content_hash.clone();
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join(".claude/skills/replace-library");
    fs::create_dir_all(&source).expect("create project skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: replace-library\ndescription: New\n---\n\nNew bytes.\n",
    )
    .expect("write new manifest");
    fs::write(source.join("unknown.txt"), "unknown metadata").expect("write extra content");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "replace-library")
        .expect("finding");
    assert!(matches!(
        finding.library_match,
        cc_switch_lib::ProjectSkillImportLibraryMatch::Different { .. }
    ));
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::ReplaceLibrary {
                library_skill_id: existing.id.clone(),
                confirmed: true,
            },
        })
        .expect("replace Library snapshot");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::LibraryReplaced
    );
    assert_eq!(
        result.library_skill_id.as_deref(),
        Some(existing.id.as_str())
    );
    let updated = state
        .db
        .get_library_skill_by_id(&existing.id)
        .expect("read updated Library")
        .expect("updated row");
    assert_eq!(updated.id, existing.id);
    assert_ne!(updated.content_hash, old_hash);
    assert_eq!(
        updated.source.kind,
        cc_switch_lib::LibrarySourceKind::LocalImport
    );
    assert_eq!(updated.description.as_deref(), Some("New"));
    assert!(
        source.is_dir(),
        "import-only must leave project source untouched"
    );
}

#[test]
fn replacement_db_failure_restores_the_old_library_snapshot() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let old_source = tempfile::tempdir().expect("old Library source");
    let old_manifest = "---\nname: swap-back\ndescription: Old\n---\n\nOld bytes.\n";
    fs::write(old_source.path().join("SKILL.md"), old_manifest).expect("write old manifest");
    let state = create_test_state().expect("create test state");
    let existing = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        old_source.path(),
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::Git,
            url: Some("https://example.invalid/swap-back".to_string()),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("swap-back"),
    )
    .expect("acquire old Library");
    let project = tempfile::tempdir().expect("project root");
    let source = project.path().join(".claude/skills/swap-back");
    fs::create_dir_all(&source).expect("create project Skill");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: swap-back\ndescription: New\n---\n\nNew bytes.\n",
    )
    .expect("write new manifest");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "swap-back")
        .expect("finding");
    state
        .db
        .fail_library_skill_updates_for_test()
        .expect("inject replacement update failure");
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportOnly,
            resolution: cc_switch_lib::ProjectSkillImportResolution::ReplaceLibrary {
                library_skill_id: existing.id.clone(),
                confirmed: true,
            },
        })
        .expect("structured replacement failure");
    assert_eq!(
        result.outcome,
        cc_switch_lib::ProjectSkillImportOutcome::Blocked
    );
    let library = state
        .db
        .get_library_skill_by_id(&existing.id)
        .expect("read old row")
        .expect("old row");
    assert_eq!(library.content_hash, existing.content_hash);
    let home = std::env::var("CC_SWITCH_TEST_HOME").expect("test home");
    assert_eq!(
        fs::read_to_string(Path::new(&home).join(".cc-switch/skills/swap-back/SKILL.md"))
            .expect("read restored Library"),
        old_manifest
    );
    assert!(source.is_dir(), "import-only source remains untouched");
}

#[test]
fn nested_git_metadata_fails_closed_as_tracked_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let project = tempfile::tempdir().expect("project root");
    Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(project.path())
        .status()
        .expect("git init");
    let source = project.path().join(".claude/skills/nested-repository");
    fs::create_dir_all(source.join(".git")).expect("create nested repository marker");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: nested-repository\ndescription: Nested repository\n---\n\nBody.\n",
    )
    .expect("write nested Skill");

    let state = create_test_state().expect("create test state");
    let workspace = cc_switch_lib::ProjectWorkspaceService::new(state.db.clone())
        .register(project.path(), None)
        .expect("register project")
        .workspace;
    let service = ProjectSkillImportService::new(state.db.clone());
    let inspection = service.inspect(&workspace.id).expect("inspect imports");
    let finding = inspection
        .findings
        .iter()
        .find(|finding| finding.directory == "nested-repository")
        .expect("nested repository finding");
    assert!(finding.git.tracked);
    assert!(finding
        .git
        .paths
        .iter()
        .any(|path| path.contains("nested repository")));
    let result = service
        .apply(cc_switch_lib::ProjectSkillImportIntent {
            workspace_id: workspace.id,
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: cc_switch_lib::ProjectSkillImportMode::ImportAndReplace,
            resolution: cc_switch_lib::ProjectSkillImportResolution::CreateNew {
                directory: "nested-repository".to_string(),
                display_name: None,
            },
        })
        .expect("structured nested repository blocker");
    assert_eq!(
        result.reason,
        Some(cc_switch_lib::ProjectSkillImportReplaceBlockReason::GitTrackedContent)
    );
    assert!(source.is_dir());
}

#[test]
fn zip_existing_identical_item_is_not_removed_when_a_later_item_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let source = tempfile::tempdir().expect("existing source");
    let manifest = b"---\nname: existing\ndescription: Existing\n---\n\nExisting bytes.\n";
    fs::write(source.path().join("SKILL.md"), manifest).expect("write existing manifest");
    let state = create_test_state().expect("create test state");
    let existing = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        source.path(),
        cc_switch_lib::LibrarySkillSource {
            kind: cc_switch_lib::LibrarySourceKind::Zip,
            url: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            skill_path: None,
            marketplace: None,
        },
        Some("existing-zip"),
    )
    .expect("seed existing Library Skill");
    let home = std::env::var("CC_SWITCH_TEST_HOME").expect("test home");
    let existing_path = Path::new(&home)
        .join(".cc-switch/skills")
        .join(&existing.directory);
    let fixtures = tempfile::tempdir().expect("fixtures");
    let zip_path = fixtures.path().join("mixed.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).expect("create ZIP"));
    let options = zip::write::SimpleFileOptions::default();
    archive
        .start_file("first-identical/SKILL.md", options)
        .expect("start identical item");
    archive.write_all(manifest).expect("write identical item");
    archive
        .start_file("second-new/SKILL.md", options)
        .expect("start new item");
    archive
        .write_all(b"---\nname: second\ndescription: Second\n---\n\nSecond bytes.\n")
        .expect("write new item");
    archive.finish().expect("finish ZIP");

    state
        .db
        .fail_library_skill_inserts_for_test()
        .expect("inject later Library insert failure");
    let error = cc_switch_lib::LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect_err("later ZIP item should fail");
    assert!(error
        .to_string()
        .contains("injected Library Skill insert failure"));
    let skills = state.db.list_library_skills().expect("list Library Skills");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].id, existing.id);
    assert_eq!(
        fs::read(existing_path.join("SKILL.md")).expect("read existing"),
        manifest
    );
}
