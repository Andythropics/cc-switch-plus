#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};

use cc_switch_lib::{
    DeploymentConsumer, GlobalSkillImportIntent, GlobalSkillImportMode, GlobalSkillImportOutcome,
    GlobalSkillImportResolution, GlobalSkillImportService, WorkspaceKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(path: &Path, name: &str, body: &str) {
    fs::create_dir_all(path).expect("create Skill directory");
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test {name}\n---\n\n{body}\n"),
    )
    .expect("write SKILL.md");
}

fn global_root(consumer: DeploymentConsumer) -> PathBuf {
    match consumer {
        DeploymentConsumer::Claude => ensure_test_home().join(".claude/skills"),
        DeploymentConsumer::Codex => ensure_test_home().join(".agents/skills"),
    }
}

#[test]
fn inspect_is_read_only_and_reports_only_real_direct_children_from_both_roots() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = create_test_state().expect("create test state");
    let service = GlobalSkillImportService::new(state.db.clone());

    let empty = service.inspect().expect("inspect missing roots");
    assert!(empty.findings.is_empty());
    assert!(!global_root(DeploymentConsumer::Claude).exists());
    assert!(!global_root(DeploymentConsumer::Codex).exists());

    let claude = global_root(DeploymentConsumer::Claude);
    let codex = global_root(DeploymentConsumer::Codex);
    write_skill(&claude.join("same-name"), "same-name", "Claude bytes");
    write_skill(&codex.join("same-name"), "same-name", "Codex bytes");
    write_skill(&claude.join(".hidden"), "hidden", "Hidden");
    fs::create_dir_all(claude.join("no-manifest")).expect("create no-manifest");
    write_skill(&claude.join("container/nested"), "nested", "Nested child");
    let external = tempfile::tempdir().expect("external Skill");
    write_skill(external.path(), "linked", "Linked");
    std::os::unix::fs::symlink(external.path(), claude.join("linked"))
        .expect("create linked child");

    let inspection = service.inspect().expect("inspect Global imports");
    assert_eq!(inspection.findings.len(), 2);
    assert_eq!(
        inspection
            .findings
            .iter()
            .filter(|finding| finding.directory == "same-name")
            .count(),
        2
    );
    assert!(inspection
        .findings
        .iter()
        .any(|finding| finding.consumer == DeploymentConsumer::Claude));
    assert!(inspection
        .findings
        .iter()
        .any(|finding| finding.consumer == DeploymentConsumer::Codex));
    assert_ne!(inspection.findings[0].id, inspection.findings[1].id);
    assert!(state
        .db
        .list_library_skills()
        .expect("list Library")
        .is_empty());
}

#[test]
fn inspect_rejects_a_symlinked_consumer_ancestor() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let external = tempfile::tempdir().expect("external Consumer root");
    let external_skill = external.path().join("skills/outside");
    write_skill(&external_skill, "outside", "Must remain outside");
    std::os::unix::fs::symlink(external.path(), ensure_test_home().join(".agents"))
        .expect("create symlinked Consumer ancestor");
    let state = create_test_state().expect("create test state");

    let error = GlobalSkillImportService::new(state.db.clone())
        .inspect()
        .expect_err("symlinked Consumer ancestor must be rejected");

    assert!(error.to_string().contains("must be a real directory"));
    assert!(external_skill.is_dir());
    assert!(state
        .db
        .list_library_skills()
        .expect("list Library")
        .is_empty());
}

#[test]
fn import_only_admits_a_copy_and_rejects_a_stale_observation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let source = global_root(DeploymentConsumer::Codex).join("global-copy");
    write_skill(&source, "global-copy", "Original");
    let state = create_test_state().expect("create test state");
    let service = GlobalSkillImportService::new(state.db.clone());
    let inspection = service.inspect().expect("inspect Global imports");
    let finding = inspection.findings[0].clone();

    fs::write(source.join("extra.txt"), "changed").expect("change after preview");
    let stale = service
        .apply(GlobalSkillImportIntent {
            finding_id: finding.id.clone(),
            observation_token: inspection.observation_token,
            mode: GlobalSkillImportMode::ImportOnly,
            resolution: GlobalSkillImportResolution::CreateNew {
                directory: "global-copy".to_string(),
                display_name: None,
            },
        })
        .expect("return stale result");
    assert_eq!(stale.outcome, GlobalSkillImportOutcome::Stale);
    assert!(state
        .db
        .list_library_skills()
        .expect("list Library")
        .is_empty());

    let fresh = service.inspect().expect("refresh observation");
    let result = service
        .apply(GlobalSkillImportIntent {
            finding_id: fresh.findings[0].id.clone(),
            observation_token: fresh.observation_token,
            mode: GlobalSkillImportMode::ImportOnly,
            resolution: GlobalSkillImportResolution::CreateNew {
                directory: "global-copy".to_string(),
                display_name: None,
            },
        })
        .expect("import copy");
    assert_eq!(result.outcome, GlobalSkillImportOutcome::Created);
    assert!(source.is_dir());
    assert!(!source.is_symlink());
    assert_eq!(
        state.db.list_library_skills().expect("list Library").len(),
        1
    );
}

#[test]
fn import_and_replace_backs_up_external_directory_and_creates_global_deployment() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let source = global_root(DeploymentConsumer::Claude).join("managed-global");
    write_skill(&source, "managed-global", "Managed bytes");
    fs::write(source.join("asset.bin"), [0_u8, 7, 255]).expect("write asset");
    let state = create_test_state().expect("create test state");
    let service = GlobalSkillImportService::new(state.db.clone());
    let inspection = service.inspect().expect("inspect Global imports");
    let finding = inspection.findings[0].clone();
    let result = service
        .apply(GlobalSkillImportIntent {
            finding_id: finding.id,
            observation_token: inspection.observation_token,
            mode: GlobalSkillImportMode::ImportAndReplace,
            resolution: GlobalSkillImportResolution::CreateNew {
                directory: "managed-global".to_string(),
                display_name: None,
            },
        })
        .expect("import and replace");

    assert_eq!(result.outcome, GlobalSkillImportOutcome::Deployed);
    assert!(source.is_symlink());
    let target = fs::read_link(&source).expect("read managed link");
    assert!(target.is_absolute());
    assert!(target.ends_with(".cc-switch/skills/managed-global"));
    let backup = result.backup_path.expect("backup path");
    assert_eq!(
        fs::read(Path::new(&backup).join("source/asset.bin")).expect("read backup asset"),
        vec![0, 7, 255]
    );
    let desired = state.db.list_skill_deployments().expect("list desired");
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].target.consumer, DeploymentConsumer::Claude);
    assert_eq!(desired[0].target.workspace, WorkspaceKind::Global);
    assert!(desired[0].target.workspace_id.is_empty());

    let after = service.inspect().expect("inspect after management");
    assert!(after.findings.is_empty(), "managed links are not unmanaged");
}

#[test]
fn deployment_failure_restores_external_source_and_library_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let source = global_root(DeploymentConsumer::Codex).join("global-rollback");
    write_skill(&source, "global-rollback", "Rollback bytes");
    let state = create_test_state().expect("create test state");
    let service = GlobalSkillImportService::new(state.db.clone());
    let inspection = service.inspect().expect("inspect Global imports");
    let finding = inspection.findings[0].clone();
    state
        .db
        .fail_skill_deployment_inserts_for_test()
        .expect("inject desired-state failure");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(true);
    let result = service
        .apply(GlobalSkillImportIntent {
            finding_id: finding.id,
            observation_token: inspection.observation_token,
            mode: GlobalSkillImportMode::ImportAndReplace,
            resolution: GlobalSkillImportResolution::CreateNew {
                directory: "global-rollback".to_string(),
                display_name: None,
            },
        })
        .expect("return compensated result");
    cc_switch_lib::SkillDeploymentService::force_compensation_failure_for_test(false);

    assert!(matches!(
        result.outcome,
        GlobalSkillImportOutcome::RolledBack | GlobalSkillImportOutcome::RecoveryRequired
    ));
    assert!(source.is_dir());
    assert!(!source.is_symlink());
    assert!(state
        .db
        .list_library_skills()
        .expect("list Library")
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
}
