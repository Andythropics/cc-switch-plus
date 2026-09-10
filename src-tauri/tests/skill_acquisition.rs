#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;

use cc_switch_lib::{
    ActivityDetailCode, ActivityOperation, ActivityOutcome, ActivityQuery, ActivityReason,
    ActivityRecorder, DiscoverableSkill, LibrarySkillAcquisitionService, LibrarySkillSource,
    LibrarySourceKind,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("assets")).expect("create Skill tree");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: careful-review\ndescription: Review a change carefully\nunknown-field: preserved\n---\n\nKeep this body byte-for-byte.\n",
    )
    .expect("write SKILL.md");
    fs::write(dir.join("assets").join("checklist.md"), "- verify\n")
        .expect("write supporting file");

    #[cfg(unix)]
    std::os::unix::fs::symlink("assets/checklist.md", dir.join("checklist.md"))
        .expect("create relative internal symlink");
}

#[test]
fn activity_append_failure_does_not_change_successful_acquisition() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source = source_root.path().join("best-effort-activity");
    write_skill(&source);
    let state = create_test_state().expect("create test state");
    state
        .db
        .fail_skill_activity_inserts_for_test()
        .expect("install activity append failpoint");

    let acquired = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        zip_source(),
        Some("best-effort-activity"),
    )
    .expect("activity persistence must not change acquisition success");

    assert_eq!(acquired.directory, "best-effort-activity");
    assert!(state
        .db
        .get_library_skill_by_id(&acquired.id)
        .expect("query acquired Skill")
        .is_some());
    assert!(ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery::default())
        .expect("list activity after injected append failure")
        .entries
        .is_empty());
}

fn zip_source() -> LibrarySkillSource {
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

#[test]
fn acquisition_admits_a_valid_skill_without_deploying_it() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source = source_root.path().join("review-skill");
    write_skill(&source);
    let state = create_test_state().expect("create test state");

    let acquired = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        zip_source(),
        None,
    )
    .expect("acquire valid Skill");

    assert!(!acquired.id.is_empty(), "Library identity must be durable");
    assert_eq!(acquired.directory, "review-skill");
    assert_eq!(acquired.display_name, "careful-review");
    assert!(acquired.compatibility.claude.compatible);
    assert!(acquired.compatibility.codex.compatible);

    let reused = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        zip_source(),
        Some("review-skill-copy"),
    )
    .expect("reuse identical content");
    assert_eq!(reused.id, acquired.id);
    assert!(!home.join(".cc-switch/skills/review-skill-copy").exists());

    let library_skill = home.join(".cc-switch").join("skills").join("review-skill");
    assert_eq!(
        fs::read_to_string(library_skill.join("SKILL.md")).expect("read preserved SKILL.md"),
        fs::read_to_string(source.join("SKILL.md")).expect("read source SKILL.md")
    );
    assert_eq!(
        fs::read_to_string(library_skill.join("assets").join("checklist.md"))
            .expect("read supporting file"),
        "- verify\n"
    );
    #[cfg(unix)]
    assert!(
        fs::symlink_metadata(library_skill.join("checklist.md"))
            .expect("read copied link")
            .file_type()
            .is_symlink(),
        "relative internal links must remain links"
    );

    let listed = state.db.list_library_skills().expect("list Library");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, acquired.id);
    assert!(
        state
            .db
            .get_all_installed_skills()
            .expect("query legacy deployments")
            .is_empty(),
        "acquisition must not create a legacy enabled/deployment row"
    );
    assert!(!home.join(".claude").exists());
    assert!(!home.join(".codex").exists());
    assert!(!home.join(".agents").exists());
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Acquire),
            ..ActivityQuery::default()
        })
        .expect("list acquisition activity");
    assert_eq!(activity.entries.len(), 2);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::NoOp);
    assert_eq!(activity.entries[1].outcome, ActivityOutcome::Success);
    assert_eq!(
        activity.entries[0].target.library_skill_id.as_deref(),
        Some(acquired.id.as_str())
    );
}

#[test]
fn display_metadata_is_editable_but_directory_identity_and_source_are_not() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let source_root = tempfile::tempdir().expect("create source root");
    let source = source_root.path().join("review-skill");
    write_skill(&source);
    let state = create_test_state().expect("create test state");
    let acquired = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &source,
        zip_source(),
        None,
    )
    .expect("acquire Skill");

    let updated = LibrarySkillAcquisitionService::update_display_metadata(
        &state.db,
        &acquired.id,
        "Review teammate",
        Some("Team review workflow"),
    )
    .expect("update display metadata");

    assert_eq!(updated.display_name, "Review teammate");
    assert_eq!(updated.description.as_deref(), Some("Team review workflow"));
    assert_eq!(updated.directory, acquired.directory);
    assert_eq!(updated.source, acquired.source);
    assert_eq!(updated.content_hash, acquired.content_hash);
    assert_eq!(
        fs::read_to_string(ensure_test_home().join(".cc-switch/skills/review-skill/SKILL.md"))
            .expect("read admitted manifest"),
        fs::read_to_string(source.join("SKILL.md")).expect("read source manifest")
    );
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::MetadataUpdate),
            ..ActivityQuery::default()
        })
        .expect("list metadata activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Success);
    assert_eq!(
        activity.entries[0].target.library_skill_id.as_deref(),
        Some(acquired.id.as_str())
    );
}

#[test]
fn malformed_metadata_id_records_targetless_typed_failure() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let state = create_test_state().expect("create test state");

    assert!(LibrarySkillAcquisitionService::update_display_metadata(
        &state.db,
        "../untrusted-id",
        "Updated",
        None,
    )
    .is_err());

    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::MetadataUpdate),
            ..ActivityQuery::default()
        })
        .expect("list failed metadata activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Failed);
    assert!(activity.entries[0].target.library_skill_id.is_none());
}

#[test]
fn admission_rejects_absolute_and_broken_internal_symlinks() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let state = create_test_state().expect("create test state");

    for (directory, target, expected) in [
        ("absolute-link", "/tmp/outside", "absolute internal symlink"),
        ("broken-link", "missing.md", "broken internal symlink"),
    ] {
        let source = fixtures.path().join(directory);
        write_skill(&source);
        fs::remove_file(source.join("checklist.md")).expect("remove valid link");
        std::os::unix::fs::symlink(target, source.join("checklist.md"))
            .expect("create unsafe link");

        let error = LibrarySkillAcquisitionService::acquire_from_directory(
            &state.db,
            &source,
            zip_source(),
            None,
        )
        .expect_err("unsafe link must reject admission");
        assert!(error.to_string().contains(expected), "unexpected: {error}");
    }

    assert!(state.db.list_library_skills().unwrap().is_empty());
}

#[test]
fn admission_rejects_noncanonical_or_incompatible_manifests() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let state = create_test_state().expect("create test state");

    for (directory, manifest) in [
        ("missing-frontmatter", "# No front matter\n"),
        (
            "invalid-name",
            "---\nname: Invalid Name\ndescription: bad\n---\n",
        ),
    ] {
        let source = fixtures.path().join(directory);
        fs::create_dir_all(&source).expect("create invalid Skill");
        fs::write(source.join("SKILL.md"), manifest).expect("write invalid manifest");
        let error = LibrarySkillAcquisitionService::acquire_from_directory(
            &state.db,
            &source,
            zip_source(),
            None,
        )
        .expect_err("incompatible manifest must reject admission");
        assert!(error.to_string().contains("incompatible"));
    }

    let no_manifest = fixtures.path().join("no-manifest");
    fs::create_dir_all(&no_manifest).unwrap();
    let error = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &no_manifest,
        zip_source(),
        None,
    )
    .expect_err("missing canonical entry must reject admission");
    assert!(error.to_string().contains("canonical SKILL.md"));
    assert!(state.db.list_library_skills().unwrap().is_empty());
}

#[test]
fn git_and_marketplace_snapshots_converge_on_the_same_library_admission() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let repo = tempfile::tempdir().expect("create repository snapshot");
    let source = repo.path().join("skills/review-skill");
    write_skill(&source);
    let state = create_test_state().expect("create test state");
    let discovered = DiscoverableSkill {
        key: "owner/repo:skills/review-skill".to_string(),
        name: "careful-review".to_string(),
        description: "Review a change carefully".to_string(),
        directory: "skills/review-skill".to_string(),
        readme_url: Some(
            "https://github.com/owner/repo/blob/missing/skills/review-skill/SKILL.md".to_string(),
        ),
        repo_owner: "owner".to_string(),
        repo_name: "repo".to_string(),
        repo_branch: "main".to_string(),
    };

    for (kind, directory) in [
        (LibrarySourceKind::Git, "review-git"),
        (LibrarySourceKind::Marketplace, "review-marketplace"),
    ] {
        if kind == LibrarySourceKind::Marketplace {
            fs::write(source.join("assets/checklist.md"), "- marketplace\n")
                .expect("make marketplace snapshot distinct");
        }
        let acquired = LibrarySkillAcquisitionService::acquire_from_repository_snapshot(
            &state.db,
            repo.path(),
            &discovered,
            kind,
            "main",
            Some(directory),
        )
        .expect("admit repository snapshot");
        assert_eq!(acquired.directory, directory);
        assert_eq!(acquired.source.kind, kind);
        assert_eq!(
            acquired.source.skill_path.as_deref(),
            Some("skills/review-skill")
        );
        assert_eq!(
            acquired.source.marketplace.as_deref(),
            (kind == LibrarySourceKind::Marketplace).then_some("skills.sh")
        );
        assert!(
            acquired
                .source
                .url
                .as_deref()
                .is_some_and(|url| url.contains("/blob/main/skills/review-skill/SKILL.md")),
            "source URL must reflect the resolved branch"
        );
    }

    assert_eq!(state.db.list_library_skills().unwrap().len(), 2);
    assert!(!home.join(".claude").exists());
    assert!(!home.join(".codex").exists());
    assert!(!home.join(".agents").exists());
}

#[test]
fn repository_snapshot_failure_and_reuse_are_recorded_once_with_typed_outcomes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let repo = tempfile::tempdir().expect("create repository snapshot");
    let source = repo.path().join("skills/review-skill");
    write_skill(&source);
    let state = create_test_state().expect("create test state");
    let discovered = DiscoverableSkill {
        key: "owner/repo:skills/review-skill".to_string(),
        name: "careful-review".to_string(),
        description: "Review a change carefully".to_string(),
        directory: "skills/review-skill".to_string(),
        readme_url: None,
        repo_owner: "owner".to_string(),
        repo_name: "repo".to_string(),
        repo_branch: "main".to_string(),
    };
    let first = LibrarySkillAcquisitionService::acquire_from_repository_snapshot(
        &state.db,
        repo.path(),
        &discovered,
        LibrarySourceKind::Git,
        "main",
        Some("repo-activity"),
    )
    .expect("create from repository snapshot");
    let reused = LibrarySkillAcquisitionService::acquire_from_repository_snapshot(
        &state.db,
        repo.path(),
        &discovered,
        LibrarySourceKind::Git,
        "main",
        Some("unused-on-reuse"),
    )
    .expect("reuse repository snapshot");
    assert_eq!(first.id, reused.id);

    let invalid = LibrarySkillAcquisitionService::acquire_from_repository_snapshot(
        &state.db,
        repo.path(),
        &discovered,
        LibrarySourceKind::Zip,
        "main",
        None,
    );
    assert!(invalid.is_err());
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Acquire),
            ..ActivityQuery::default()
        })
        .expect("list repository acquisition activity");
    assert_eq!(activity.entries.len(), 3);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Failed);
    assert!(activity.entries[0].target.library_skill_id.is_none());
    assert_eq!(activity.entries[1].outcome, ActivityOutcome::NoOp);
    assert_eq!(activity.entries[2].outcome, ActivityOutcome::Success);
}

#[test]
fn malformed_zip_records_one_targetless_failed_acquisition() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("malformed.zip");
    fs::write(&zip_path, b"not a zip").expect("write malformed archive");
    let state = create_test_state().expect("create test state");

    assert!(LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .is_err());
    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Acquire),
            ..ActivityQuery::default()
        })
        .expect("list malformed ZIP activity");
    assert_eq!(activity.entries.len(), 1);
    assert_eq!(activity.entries[0].outcome, ActivityOutcome::Failed);
    assert!(activity.entries[0].target.library_skill_id.is_none());
}

fn write_skill_zip(path: &std::path::Path, link_target: &str, checklist: &[u8]) {
    use zip::write::SimpleFileOptions;

    let file = fs::File::create(path).expect("create ZIP");
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .add_directory("review-skill/assets/", options)
        .unwrap();
    archive
        .start_file("review-skill/SKILL.md", options)
        .unwrap();
    archive
        .write_all(
            b"---\nname: careful-review\ndescription: Review a change carefully\nunknown-field: preserved\n---\n",
        )
        .unwrap();
    archive
        .start_file("review-skill/assets/checklist.md", options)
        .unwrap();
    archive.write_all(checklist).unwrap();
    archive
        .add_symlink("review-skill/checklist.md", link_target, options)
        .unwrap();
    archive.finish().unwrap();
}

#[test]
fn zip_acquisition_preserves_links_and_requires_an_explicit_unique_identity() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("skill.zip");
    write_skill_zip(&zip_path, "assets/checklist.md", b"- verify\n");
    let state = create_test_state().expect("create test state");

    let acquired = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect("acquire ZIP");
    assert_eq!(acquired.len(), 1);
    assert!(fs::symlink_metadata(
        home.join(".cc-switch")
            .join("skills")
            .join("review-skill")
            .join("checklist.md")
    )
    .expect("read Library link")
    .file_type()
    .is_symlink());

    let repeated = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect("same ZIP must reuse before checking occupied directory");
    assert_eq!(repeated[0].id, acquired[0].id);
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);

    write_skill_zip(&zip_path, "assets/checklist.md", b"- changed\n");

    let conflict = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect_err("collision must require an explicit identity");
    assert!(conflict.to_string().contains("LIBRARY_DIRECTORY_CONFLICT"));

    let renamed = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::from([(
            "review-skill".to_string(),
            "review-skill-2".to_string(),
        )]),
    )
    .expect("acquire with explicit unique identity");
    assert_eq!(renamed[0].directory, "review-skill-2");
}

#[test]
fn zip_acquisition_rejects_an_escaping_internal_symlink() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("unsafe.zip");
    write_skill_zip(&zip_path, "../../outside.md", b"- verify\n");
    let state = create_test_state().expect("create test state");

    let error = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect_err("escaping link must reject admission");
    assert!(error.to_string().contains("symlink"));
    assert!(state.db.list_library_skills().unwrap().is_empty());
}

#[test]
fn root_level_zip_skill_uses_the_archive_name_as_its_directory_identity() {
    use zip::write::SimpleFileOptions;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("root-skill.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    archive
        .start_file("SKILL.md", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(b"---\nname: root-skill\ndescription: Root-level Skill\n---\n")
        .unwrap();
    archive.finish().unwrap();
    let state = create_test_state().expect("create test state");

    let acquired = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect("acquire root Skill");

    assert_eq!(acquired[0].directory, "root-skill");
}

#[test]
fn multi_skill_zip_preflights_all_collisions_before_writing_any_skill() {
    use zip::write::SimpleFileOptions;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let existing = fixtures.path().join("b-skill");
    write_skill(&existing);
    let state = create_test_state().expect("create test state");
    LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        &existing,
        zip_source(),
        None,
    )
    .expect("seed collision");

    let zip_path = fixtures.path().join("two-skills.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    for directory in ["a-skill", "b-skill"] {
        archive
            .start_file(
                format!("{directory}/SKILL.md"),
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(format!("---\nname: {directory}\ndescription: Test Skill\n---\n").as_bytes())
            .unwrap();
    }
    archive.finish().unwrap();

    let error = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect_err("batch collision must reject before admission");

    assert!(error.to_string().contains("LIBRARY_DIRECTORY_CONFLICT"));
    assert!(!home.join(".cc-switch/skills/a-skill").exists());
    assert_eq!(state.db.list_library_skills().unwrap().len(), 1);
}

#[test]
fn multi_skill_zip_records_one_ordered_activity_item_per_admission() {
    use zip::write::SimpleFileOptions;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("ordered-skills.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    for (directory, body) in [("a-skill", "first"), ("b-skill", "second")] {
        archive
            .start_file(
                format!("{directory}/SKILL.md"),
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(
                format!("---\nname: {directory}\ndescription: Test Skill\n---\n\n{body}\n")
                    .as_bytes(),
            )
            .unwrap();
    }
    archive.finish().unwrap();
    let state = create_test_state().expect("create test state");

    let acquired = LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .expect("acquire ordered ZIP");
    assert_eq!(acquired.len(), 2);

    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Acquire),
            ..ActivityQuery::default()
        })
        .expect("list ZIP activity");
    assert_eq!(activity.entries.len(), 2);
    assert_eq!(
        activity.entries[0].target.library_skill_id.as_deref(),
        Some(acquired[1].id.as_str())
    );
    assert_eq!(
        activity.entries[1].target.library_skill_id.as_deref(),
        Some(acquired[0].id.as_str())
    );
    let newest = activity.entries[0].batch.as_ref().expect("ZIP batch");
    let oldest = activity.entries[1].batch.as_ref().expect("ZIP batch");
    assert_eq!(newest.batch_id, oldest.batch_id);
    assert_eq!(newest.item_index, 1);
    assert_eq!(oldest.item_index, 0);
    assert_eq!(newest.item_count, 2);
}

#[test]
fn late_zip_failure_records_ordered_rollback_failure_and_unattempted_batch_items() {
    use zip::write::SimpleFileOptions;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let zip_path = fixtures.path().join("late-failure.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    for directory in ["a-created", "b-fails", "c-unattempted"] {
        archive
            .start_file(
                format!("{directory}/SKILL.md"),
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(
                format!(
                    "---\nname: {directory}\ndescription: ZIP late failure fixture\n---\n\n{directory}\n"
                )
                .as_bytes(),
            )
            .unwrap();
    }
    archive.finish().unwrap();
    let state = create_test_state().expect("create test state");
    state
        .db
        .fail_library_skill_inserts_after_for_test(1)
        .expect("fail the second Library insert");

    assert!(LibrarySkillAcquisitionService::acquire_from_zip(
        &state.db,
        &zip_path,
        &std::collections::HashMap::new(),
    )
    .is_err());
    assert!(state.db.list_library_skills().unwrap().is_empty());

    let activity = ActivityRecorder::new(state.db.clone())
        .list(ActivityQuery {
            operation: Some(ActivityOperation::Library),
            reason: Some(ActivityReason::Acquire),
            ..ActivityQuery::default()
        })
        .expect("list late ZIP failure activity");
    assert_eq!(activity.entries.len(), 3, "must not add an aggregate row");
    let by_index = |index| {
        activity
            .entries
            .iter()
            .find(|entry| {
                entry
                    .batch
                    .as_ref()
                    .is_some_and(|batch| batch.item_index == index)
            })
            .expect("activity item by batch index")
    };
    let batch_id = by_index(0).batch.as_ref().unwrap().batch_id.clone();
    assert!(activity.entries.iter().all(|entry| {
        entry
            .batch
            .as_ref()
            .is_some_and(|batch| batch.batch_id == batch_id && batch.item_count == 3)
    }));
    assert_eq!(by_index(0).outcome, ActivityOutcome::RolledBack);
    assert!(by_index(0).target.library_skill_id.is_some());
    assert_eq!(by_index(1).outcome, ActivityOutcome::Failed);
    assert_eq!(by_index(1).detail_code, ActivityDetailCode::DatabaseFailure);
    assert!(by_index(1).target.library_skill_id.is_none());
    assert_eq!(by_index(2).outcome, ActivityOutcome::Blocked);
    assert_eq!(by_index(2).detail_code, ActivityDetailCode::PartialBatch);
    assert!(by_index(2).target.library_skill_id.is_none());
    assert!(!activity
        .entries
        .iter()
        .any(|entry| entry.outcome == ActivityOutcome::Success));
}

#[test]
fn historical_baseline_cannot_reuse_modified_or_missing_library_content() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ensure_test_home();
    let source = tempfile::tempdir().unwrap();
    write_skill(source.path());
    let state = create_test_state().unwrap();
    let existing = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        source.path(),
        zip_source(),
        Some("original"),
    )
    .unwrap();
    let live = home.join(".cc-switch/skills/original");
    fs::write(
        live.join("SKILL.md"),
        "---\nname: changed\ndescription: Changed\n---\nChanged",
    )
    .unwrap();
    let second = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        source.path(),
        zip_source(),
        Some("second"),
    )
    .expect("historical baseline cannot prevent a distinct current snapshot");
    assert_ne!(second.id, existing.id);
    assert_eq!(
        fs::read(home.join(".cc-switch/skills/second/SKILL.md")).unwrap(),
        fs::read(source.path().join("SKILL.md")).unwrap()
    );
    fs::remove_dir_all(&live).unwrap();
    fs::remove_dir_all(home.join(".cc-switch/skills/second")).unwrap();
    let third = LibrarySkillAcquisitionService::acquire_from_directory(
        &state.db,
        source.path(),
        zip_source(),
        Some("third"),
    )
    .expect("missing snapshots must not count as identical");
    assert_ne!(third.id, existing.id);
    assert_ne!(third.id, second.id);
    assert_eq!(state.db.list_library_skills().unwrap().len(), 3);
}
