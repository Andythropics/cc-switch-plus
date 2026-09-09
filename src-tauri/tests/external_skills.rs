#![cfg(target_os = "macos")]
use cc_switch_lib::{
    DeploymentBatch, DeploymentConsumer, DeploymentIntent, DeploymentTarget,
    LibrarySkillAcquisitionService, LibrarySkillSource, LibrarySkillUpdateApplyOutcome,
    LibrarySkillUpdateReason, LibrarySourceKind, SkillDeploymentService,
};
use cc_switch_lib::{ExternalSkillIntent, ExternalSkillService};
use std::{fs, path::Path};
#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};
fn write_skill(path: &Path, body: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: implementation\ndescription: Test fixture\n---\n{body}\n"),
    )
    .unwrap();
}
fn local_source() -> LibrarySkillSource {
    LibrarySkillSource {
        kind: LibrarySourceKind::LocalImport,
        url: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        skill_path: None,
        marketplace: None,
    }
}
fn external(body: &str) {
    let root = ensure_test_home().join(".agents");
    write_skill(&root.join("skills/implementation"), body);
    fs::write(root.join(".skill-lock.json"),r#"{"skills":{"implementation":{"source":"example/skills","sourceType":"github","skillPath":"skills/implementation/SKILL.md"},"stale":{"source":"example/skills","sourceType":"github","skillPath":"stale/SKILL.md"}}}"#).unwrap();
}
#[test]
fn inspection_marks_only_fully_managed_unchanged_links_as_synced() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "same");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("same");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(!scan.candidates[0].fully_synced);
    ExternalSkillService::link_external(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: false,
        },
    )
    .unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(!scan.candidates[0].changed);
    assert!(
        !scan.candidates[0].fully_synced,
        "identical hard copy still needs conversion"
    );
    let path = ensure_test_home().join(".agents/skills/implementation");
    let library_path = ensure_test_home().join(".cc-switch/skills/implementation");
    fs::remove_dir_all(&path).unwrap();
    std::os::unix::fs::symlink(&library_path, &path).unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(
        !scan.candidates[0].fully_synced,
        "unrecorded link still needs adoption"
    );
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.candidates[0].target_observation_tokens[&skill.id].clone(),
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Updated | LibrarySkillUpdateApplyOutcome::UpToDate
    ));
    assert!(ExternalSkillService::inspect(&state.db).unwrap().candidates[0].fully_synced);
    let original = fs::read(library_path.join("SKILL.md")).unwrap();
    write_skill(&library_path, "changed through link");
    assert!(!ExternalSkillService::inspect(&state.db).unwrap().candidates[0].fully_synced);
    fs::write(library_path.join("SKILL.md"), &original).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("SKILL.md"), &original).unwrap();
    assert!(
        !ExternalSkillService::inspect(&state.db).unwrap().candidates[0].fully_synced,
        "recorded deployment replaced by same-content copy still needs repair"
    );
    fs::remove_dir_all(&path).unwrap();
    std::os::unix::fs::symlink(&library_path, &path).unwrap();
    let deployment = state.db.list_skill_deployments().unwrap().remove(0);
    state.db.delete_skill_deployment(&deployment.id).unwrap();
    assert!(!ExternalSkillService::inspect(&state.db).unwrap().candidates[0].fully_synced);
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(fixture.path().join("missing"), &path).unwrap();
    let broken = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(broken.candidates.is_empty());
    assert!(broken
        .warnings
        .iter()
        .any(|warning| warning.contains("broken")));
}

#[test]
#[cfg(debug_assertions)]
fn scoped_update_hashes_only_selected_content_during_revalidation_and_replacement() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old selected");
    let selected = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("new selected");
    write_skill(fixture.path(), "unrelated");
    LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("unrelated"),
    )
    .unwrap();
    write_skill(
        &ensure_test_home().join(".agents/skills/unrelated"),
        "unrelated",
    );
    fs::write(ensure_test_home().join(".agents/.skill-lock.json"), r#"{"skills":{"implementation":{"source":"example/skills","sourceType":"github","skillPath":"implementation/SKILL.md"},"unrelated":{"source":"example/skills","sourceType":"github","skillPath":"unrelated/SKILL.md"}}}"#).unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    let token = scan
        .candidates
        .iter()
        .find(|candidate| candidate.id == "implementation")
        .unwrap()
        .target_observation_tokens[&selected.id]
        .clone();
    LibrarySkillAcquisitionService::start_hash_trace_for_test();
    let applied = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: selected.id,
            observation_token: token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    );
    let hashed = LibrarySkillAcquisitionService::take_hash_trace_for_test();
    assert_eq!(
        applied.unwrap().outcome,
        LibrarySkillUpdateApplyOutcome::Updated
    );
    assert!(
        !hashed.is_empty(),
        "selected content must still be revalidated"
    );
    assert!(
        !hashed
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "unrelated")),
        "scoped update must not hash unrelated content: {hashed:?}"
    );
}

#[test]
fn reviewed_unrelated_candidates_can_be_applied_sequentially_without_rescanning() {
    assert_reviewed_candidates_remain_actionable(false);
}

#[test]
fn linking_one_candidate_preserves_another_reviewed_candidates_observation() {
    assert_reviewed_candidates_remain_actionable(true);
}

fn assert_reviewed_candidates_remain_actionable(link_first: bool) {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let mut skills = Vec::new();
    for directory in ["first", "second"] {
        write_skill(fixture.path(), &format!("old {directory}"));
        skills.push(
            LibrarySkillAcquisitionService::acquire_from_directory(
                &state.db,
                fixture.path(),
                local_source(),
                Some(directory),
            )
            .unwrap(),
        );
        write_skill(
            &ensure_test_home().join(".agents/skills").join(directory),
            &format!("new {directory}"),
        );
    }
    fs::write(ensure_test_home().join(".agents/.skill-lock.json"), r#"{"skills":{"first":{"source":"example/skills","sourceType":"github","skillPath":"first/SKILL.md"},"second":{"source":"example/skills","sourceType":"github","skillPath":"second/SKILL.md"}}}"#).unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(
        ExternalSkillService::apply(
            &state.db,
            ExternalSkillIntent {
                candidate_id: "first".into(),
                library_skill_id: skills[1].id.clone(),
                observation_token: scan.candidates[0].target_observation_tokens[&skills[0].id]
                    .clone(),
                confirm_local_modifications: false,
                restore_deployment: true,
            }
        )
        .is_err(),
        "a scoped token must not authorize another selected Library Skill"
    );
    for (index, skill) in skills.into_iter().enumerate() {
        let token = scan
            .candidates
            .iter()
            .find(|candidate| candidate.id == skill.directory)
            .unwrap()
            .target_observation_tokens[&skill.id]
            .clone();
        let intent = ExternalSkillIntent {
            candidate_id: skill.directory,
            library_skill_id: skill.id,
            observation_token: token,
            confirm_local_modifications: false,
            restore_deployment: true,
        };
        if link_first && index == 0 {
            ExternalSkillService::link_external(&state.db, intent).unwrap();
            continue;
        }
        let result = ExternalSkillService::apply(&state.db, intent)
            .expect("another reviewed candidate must remain actionable after an unrelated update");
        assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Updated);
    }
}

#[test]
fn scoped_external_observations_reject_changes_to_the_reviewed_pair() {
    let _guard = test_mutex().lock().unwrap();
    for change in [
        "external",
        "library",
        "source",
        "deployment",
        "association",
        "physical_source",
    ] {
        reset_test_fs();
        let state = create_test_state().unwrap();
        let fixture = tempfile::tempdir().unwrap();
        write_skill(fixture.path(), "old");
        let skill = LibrarySkillAcquisitionService::acquire_from_directory(
            &state.db,
            fixture.path(),
            local_source(),
            Some("implementation"),
        )
        .unwrap();
        external("new");
        let scan = ExternalSkillService::inspect(&state.db).unwrap();
        let intent = ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.candidates[0].target_observation_tokens[&skill.id].clone(),
            confirm_local_modifications: true,
            restore_deployment: true,
        };
        match change {
            "external" => external("newer"),
            "library" => write_skill(
                &ensure_test_home().join(".cc-switch/skills/implementation"),
                "edited after review",
            ),
            "source" => {
                let lock = ensure_test_home().join(".agents/.skill-lock.json");
                let text = fs::read_to_string(&lock)
                    .unwrap()
                    .replace("example/skills", "changed/skills");
                fs::write(lock, text).unwrap();
            }
            "deployment" => {
                SkillDeploymentService::new(state.db.clone())
                    .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
                        library_skill_id: skill.id.clone(),
                        target: DeploymentTarget::global(DeploymentConsumer::Claude),
                    }))
                    .unwrap();
            }
            "association" => {
                ExternalSkillService::link_external(&state.db, intent.clone()).unwrap();
            }
            "physical_source" => {
                // Keep bytes identical while replacing the reviewed directory.
                let path = ensure_test_home().join(".agents/skills/implementation");
                fs::rename(&path, ensure_test_home().join(".agents/skills/parked")).unwrap();
                external("new");
            }
            _ => unreachable!(),
        }
        let before = state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap();
        assert!(
            ExternalSkillService::apply(&state.db, intent).is_err(),
            "{change} must invalidate its scoped observation"
        );
        assert_eq!(
            state
                .db
                .get_library_skill_by_id(&skill.id)
                .unwrap()
                .unwrap(),
            before
        );
        assert!(!ensure_test_home()
            .join(".agents/skills/implementation")
            .is_symlink());
    }
}

#[test]
fn explicit_association_preserves_identity_and_detects_stale_external_changes() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let old = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("new");
    let inspected = ExternalSkillService::inspect(&state.db).unwrap();
    assert_eq!(inspected.candidates.len(), 1);
    let c = &inspected.candidates[0];
    assert!(c.library_skill_id.is_none());
    assert_eq!(c.suggested_library_skill_ids, vec![old.id.clone()]);
    let intent = ExternalSkillIntent {
        candidate_id: c.id.clone(),
        library_skill_id: old.id.clone(),
        observation_token: inspected.observation_token,
        confirm_local_modifications: false,
        restore_deployment: false,
    };
    let linked = ExternalSkillService::link_external(&state.db, intent.clone()).unwrap();
    assert_eq!(linked.id, old.id);
    assert_eq!(linked.content_hash, old.content_hash);
    assert!(!ensure_test_home()
        .join(".agents/skills/implementation")
        .is_symlink());
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_eq!(
        linked.source.skill_path.as_deref(),
        Some("skills/implementation")
    );
    assert!(ExternalSkillService::apply(&state.db, intent).is_err());
    let inspected = ExternalSkillService::inspect(&state.db).unwrap();
    assert_eq!(
        inspected.candidates[0].library_skill_id.as_deref(),
        Some(old.id.as_str())
    );
    let intent = ExternalSkillIntent {
        candidate_id: "implementation".into(),
        library_skill_id: old.id.clone(),
        observation_token: inspected.observation_token,
        confirm_local_modifications: false,
        restore_deployment: false,
    };
    external("newer");
    assert!(ExternalSkillService::apply(&state.db, intent).is_err());
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&old.id)
            .unwrap()
            .unwrap()
            .content_hash,
        old.content_hash
    );
}
#[test]
fn first_external_update_creates_deployment_from_unmanaged_directory() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let old = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("new");
    let inspected = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(!inspected.candidates[0].deployment_replaced);
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: old.id.clone(),
            observation_token: inspected.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Updated,
        "{result:?}"
    );
    assert!(Path::new(result.backup_path.as_ref().unwrap()).exists());
    let external_path = ensure_test_home().join(".agents/skills/implementation");
    assert!(
        fs::symlink_metadata(&external_path).unwrap().file_type().is_symlink(),
        "explicit Link and Update must replace the first unmanaged installation with a Library link"
    );
    assert_eq!(
        fs::canonicalize(&external_path).unwrap(),
        fs::canonicalize(
            ensure_test_home()
                .join(".cc-switch/skills")
                .join(&old.directory)
        )
        .unwrap()
    );
    let desired = state.db.list_skill_deployments().unwrap();
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].library_skill_id, old.id);
    assert_eq!(
        desired[0].target,
        DeploymentTarget::global(DeploymentConsumer::Codex)
    );
    let link = fs::read_link(&external_path).unwrap();
    assert!(link.is_absolute());
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    let noop = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: old.id,
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        noop.outcome,
        LibrarySkillUpdateApplyOutcome::UpToDate,
        "{noop:?}"
    );
    assert_eq!(fs::read_link(external_path).unwrap(), link);
    assert_eq!(state.db.list_skill_deployments().unwrap(), desired);
}

#[test]
fn first_external_update_deploys_identical_content_without_prior_record() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "identical");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("identical");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Updated,
        "{result:?}"
    );
    assert!(ensure_test_home()
        .join(".agents/skills/implementation")
        .is_symlink());
    assert_eq!(
        state.db.list_skill_deployments().unwrap()[0].library_skill_id,
        skill.id
    );
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap()
            .source
            .kind,
        LibrarySourceKind::Git
    );
    let backups = ensure_test_home().join(".cc-switch/skill-import-backups");
    assert!(
        fs::read_dir(backups).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.join("source/SKILL.md").exists() || path.join("global-source/SKILL.md").exists()
        }),
        "CLI directory must be backed up even when Library content is already current"
    );
}

#[test]
fn external_update_blocks_directory_mismatch_before_changing_library_or_source() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("other-directory"),
    )
    .unwrap();
    external("new");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Blocked,
        "{result:?}"
    );
    assert!(result.message.unwrap().contains("immutable directory name"));
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap(),
        skill
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert!(!ensure_test_home()
        .join(".agents/skills/implementation")
        .is_symlink());
    assert!(
        fs::read_to_string(ensure_test_home().join(".agents/skills/implementation/SKILL.md"))
            .unwrap()
            .ends_with("new\n")
    );
}

#[test]
fn external_update_rejects_reviewed_directory_replaced_with_conflicting_link() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("new");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    let external_path = ensure_test_home().join(".agents/skills/implementation");
    fs::remove_dir_all(&external_path).unwrap();
    std::os::unix::fs::symlink(fixture.path(), &external_path).unwrap();
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read_link(external_path).unwrap(), fixture.path());
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap(),
        skill
    );
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
}

#[test]
fn explicit_external_update_records_an_existing_unrecorded_library_link() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "existing");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("existing");
    let path = ensure_test_home().join(".agents/skills/implementation");
    fs::remove_dir_all(&path).unwrap();
    let library_path = ensure_test_home()
        .join(".cc-switch/skills")
        .join(&skill.directory);
    std::os::unix::fs::symlink(&library_path, &path).unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(!scan.candidates[0].deployment_replaced);
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Updated,
        "{result:?}"
    );
    assert_eq!(fs::read_link(path).unwrap(), library_path);
    let desired = state.db.list_skill_deployments().unwrap();
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].library_skill_id, skill.id);
}

#[test]
fn first_external_deployment_failure_reports_recovery_and_retains_library_backup() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("new");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    state.db.fail_skill_deployment_inserts_for_test().unwrap();
    let result = ExternalSkillService::apply(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: true,
        },
    )
    .unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::RecoveryRequired,
        "{result:?}"
    );
    let message = result.message.unwrap();
    assert!(message.contains("CLI deployment needs attention"));
    let cli_backup = message.split("CLI directory backup: ").nth(1).unwrap();
    assert!(Path::new(cli_backup).join("source/SKILL.md").exists());
    assert_ne!(Some(cli_backup), result.backup_path.as_deref());
    assert!(Path::new(result.backup_path.as_ref().unwrap()).exists());
    let external_path = ensure_test_home().join(".agents/skills/implementation");
    assert!(!external_path.is_symlink());
    assert!(fs::read_to_string(external_path.join("SKILL.md"))
        .unwrap()
        .ends_with("new\n"));
    assert!(state.db.list_skill_deployments().unwrap().is_empty());
    assert_ne!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap()
            .content_hash,
        skill.content_hash
    );
}

#[test]
fn external_update_requires_local_confirmation_and_preserves_backup_and_deployment() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let old = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    let target = DeploymentTarget::global(DeploymentConsumer::Codex);
    SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: old.id.clone(),
            target,
        }))
        .unwrap();
    let external_path = ensure_test_home().join(".agents/skills/implementation");
    fs::remove_file(&external_path).unwrap();
    external("new");
    write_skill(
        &ensure_test_home()
            .join(".cc-switch/skills")
            .join(&old.directory),
        "local edited",
    );
    let inspected = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(inspected.candidates[0].deployment_replaced);
    let mut intent = ExternalSkillIntent {
        candidate_id: "implementation".into(),
        library_skill_id: old.id.clone(),
        observation_token: inspected.observation_token,
        confirm_local_modifications: false,
        restore_deployment: true,
    };
    let blocked = ExternalSkillService::apply(&state.db, intent.clone()).unwrap();
    assert_eq!(
        blocked.reason,
        Some(LibrarySkillUpdateReason::LocalModificationConfirmationRequired)
    );
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&old.id)
            .unwrap()
            .unwrap()
            .source
            .kind,
        LibrarySourceKind::LocalImport
    );
    let stages = ensure_test_home().join(".cc-switch/skill-update-stages");
    assert_eq!(
        fs::read_dir(stages).unwrap().count(),
        0,
        "blocked external update must discard its stage"
    );
    intent.confirm_local_modifications = true;
    let result = ExternalSkillService::apply(&state.db, intent).unwrap();
    assert_eq!(
        result.outcome,
        LibrarySkillUpdateApplyOutcome::Updated,
        "{:?}",
        result
    );
    assert!(Path::new(result.backup_path.as_ref().unwrap()).exists());
    assert!(fs::symlink_metadata(external_path)
        .unwrap()
        .file_type()
        .is_symlink());
    let updated = state.db.get_library_skill_by_id(&old.id).unwrap().unwrap();
    assert_eq!(updated.id, old.id);
    assert_eq!(updated.directory, old.directory);
    assert_eq!(updated.source.kind, LibrarySourceKind::Git);
    assert_ne!(updated.content_hash, old.content_hash);
}

#[test]
fn relink_invalidates_previously_staged_upstream_without_changing_content() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let local = tempfile::tempdir().unwrap();
    write_skill(local.path(), "old");
    let mut source = local_source();
    source.kind = LibrarySourceKind::Git;
    source.repo_owner = Some("a".into());
    source.repo_name = Some("skills".into());
    source.repo_branch = Some("main".into());
    source.skill_path = Some(".".into());
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        local.path(),
        source,
        Some("implementation"),
    )
    .unwrap();
    let upstream = tempfile::tempdir().unwrap();
    write_skill(upstream.path(), "upstream a");
    let staged = cc_switch_lib::LibrarySkillUpdateService::stage_from_repository_snapshot(
        &state.db,
        &skill.id,
        upstream.path(),
    )
    .unwrap();
    external("upstream b");
    let inspected = ExternalSkillService::inspect(&state.db).unwrap();
    ExternalSkillService::link_external(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: inspected.observation_token,
            confirm_local_modifications: false,
            restore_deployment: false,
        },
    )
    .unwrap();
    let result = cc_switch_lib::LibrarySkillUpdateService::apply(
        &state.db,
        cc_switch_lib::LibrarySkillUpdateApplyIntent {
            library_skill_id: skill.id.clone(),
            stage_token: staged.stage_token.unwrap(),
            observation_token: staged.observation_token,
            confirm_local_modifications: false,
        },
    )
    .unwrap();
    assert_eq!(result.outcome, LibrarySkillUpdateApplyOutcome::Stale);
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap()
            .content_hash,
        skill.content_hash
    );
}

#[test]
fn preserve_tracking_branch_reject_contradictory_link_and_stale_deployment_intent() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let a = tempfile::tempdir().unwrap();
    write_skill(a.path(), "a");
    let mut source = local_source();
    source.kind = LibrarySourceKind::Git;
    source.repo_owner = Some("example".into());
    source.repo_name = Some("skills".into());
    source.repo_branch = Some("release".into());
    source.skill_path = Some("skills/implementation".into());
    let a = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        a.path(),
        source,
        Some("implementation"),
    )
    .unwrap();
    external("new a");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert_eq!(
        scan.candidates[0].source.repo_branch.as_deref(),
        Some("release")
    );
    let intent = ExternalSkillIntent {
        candidate_id: "implementation".into(),
        library_skill_id: a.id.clone(),
        observation_token: scan.observation_token,
        confirm_local_modifications: false,
        restore_deployment: false,
    };
    SkillDeploymentService::new(state.db.clone())
        .apply(DeploymentBatch::single(DeploymentIntent::Deploy {
            library_skill_id: a.id.clone(),
            target: DeploymentTarget::global(DeploymentConsumer::Claude),
        }))
        .unwrap();
    assert!(
        ExternalSkillService::apply(&state.db, intent).is_err(),
        "changed desired deployment invalidates reviewed scan"
    );
    let b = tempfile::tempdir().unwrap();
    write_skill(b.path(), "unrelated b");
    let b = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        b.path(),
        local_source(),
        Some("other"),
    )
    .unwrap();
    let external = ensure_test_home().join(".agents/skills/implementation");
    fs::remove_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(
        ensure_test_home()
            .join(".cc-switch/skills")
            .join(b.directory),
        external,
    )
    .unwrap();
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(scan.candidates.is_empty());
    assert!(!scan.warnings.is_empty());
}

#[test]
fn unmatched_library_edits_invalidate_external_association_observation() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    write_skill(fixture.path(), "old");
    let skill = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        fixture.path(),
        local_source(),
        Some("implementation"),
    )
    .unwrap();
    external("external");
    let scan = ExternalSkillService::inspect(&state.db).unwrap();
    assert!(scan.candidates[0].library_skill_id.is_none());
    write_skill(
        &ensure_test_home()
            .join(".cc-switch/skills")
            .join(&skill.directory),
        "edited after inspection",
    );
    assert!(ExternalSkillService::link_external(
        &state.db,
        ExternalSkillIntent {
            candidate_id: "implementation".into(),
            library_skill_id: skill.id.clone(),
            observation_token: scan.observation_token,
            confirm_local_modifications: false,
            restore_deployment: false,
        },
    )
    .is_err());
    assert_eq!(
        state
            .db
            .get_library_skill_by_id(&skill.id)
            .unwrap()
            .unwrap()
            .source
            .kind,
        LibrarySourceKind::LocalImport
    );
}
