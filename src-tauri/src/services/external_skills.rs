//! Explicit reconciliation of npx snapshots with stable Library identities.
use super::skill::{
    LibrarySkill, LibrarySkillAcquisitionService as Acquisition, LibrarySkillSource,
    LibrarySourceKind, SkillService,
};
use super::skill_update::{
    LibrarySkillUpdateApplyIntent, LibrarySkillUpdateApplyOutcome, LibrarySkillUpdateReason,
    LibrarySkillUpdateResult, LibrarySkillUpdateService,
};
use crate::database::Database;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSkillCandidate {
    pub id: String,
    pub directory: String,
    pub source: LibrarySkillSource,
    pub content_hash: String,
    pub library_skill_id: Option<String>,
    pub suggested_library_skill_ids: Vec<String>,
    pub changed: bool,
    pub local_modified: bool,
    pub deployment_replaced: bool,
    /// Presentation state only: action validation still retains every candidate.
    #[serde(default)]
    pub fully_synced: bool,
    /// Observations scoped to this candidate and each selectable Library Skill.
    /// Unrelated reconciliations leave these tokens valid for the retained list.
    #[serde(default)]
    pub target_observation_tokens: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSkillInspection {
    pub observation_token: String,
    pub candidates: Vec<ExternalSkillCandidate>,
    pub warnings: Vec<String>,
}
impl ExternalSkillInspection {
    pub(crate) fn accepts_target_observation(&self, token: &str, library_skill_id: &str) -> bool {
        self.observation_token == token
            || self.candidates.iter().any(|candidate| {
                candidate
                    .target_observation_tokens
                    .get(library_skill_id)
                    .map(String::as_str)
                    == Some(token)
            })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSkillIntent {
    pub candidate_id: String,
    pub library_skill_id: String,
    pub observation_token: String,
    pub confirm_local_modifications: bool,
    /// Explicitly replace the CLI directory with a Library deployment, including
    /// the first association when no desired deployment has been recorded yet.
    #[serde(default)]
    pub restore_deployment: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkLibrarySkillSourceIntent {
    pub library_skill_id: String,
    pub source: LibrarySkillSource,
    pub expected_content_hash: String,
}
#[derive(Deserialize, Default)]
struct LockFile {
    #[serde(default)]
    skills: BTreeMap<String, LockSkill>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LockSkill {
    source: Option<String>,
    source_type: Option<String>,
    source_url: Option<String>,
    skill_path: Option<String>,
    branch: Option<String>,
    source_branch: Option<String>,
}

pub struct ExternalSkillService;
impl ExternalSkillService {
    pub fn root() -> PathBuf {
        crate::config::get_home_dir().join(".agents")
    }
    pub fn inspect(db: &Arc<Database>) -> Result<ExternalSkillInspection> {
        Self::inspect_at(db, &Self::root())
    }
    pub fn inspect_at(db: &Arc<Database>, root: &Path) -> Result<ExternalSkillInspection> {
        Self::inspect_scope_at(db, root, None)
    }

    pub(crate) fn inspect_for_observation(
        db: &Arc<Database>,
        root: &Path,
        candidate_id: Option<&str>,
        library_skill_id: &str,
        token: &str,
    ) -> Result<ExternalSkillInspection> {
        let scope = candidate_id
            .filter(|_| token.starts_with("target-v1:"))
            .map(|candidate| (candidate, library_skill_id));
        Self::inspect_scope_at(db, root, scope)
    }

    /// Initial review reads every candidate/target. Scoped action observations
    /// use exactly the same builders and digest, but read only the selected pair.
    fn inspect_scope_at(
        db: &Arc<Database>,
        root: &Path,
        scope: Option<(&str, &str)>,
    ) -> Result<ExternalSkillInspection> {
        let bytes = match fs::read(root.join(".skill-lock.json")) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => b"{}".to_vec(),
            Err(e) => return Err(e.into()),
        };
        let lock: LockFile = serde_json::from_slice(&bytes)
            .map_err(|e| anyhow!("Cannot read skills lock file: {e}"))?;
        let library = db.list_library_skills()?;
        let deployments = db.list_skill_deployments()?;
        let library_root = Acquisition::library_directory_path();
        // Caches belong to this observation only; mutation commands inspect afresh.
        let mut live_hashes = BTreeMap::new();
        let mut baseline_matches = BTreeMap::new();
        let mut library_paths = BTreeMap::new();
        let mut source_inspections = BTreeMap::new();
        let mut candidates = Vec::new();
        let mut warnings = Vec::new();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hasher.update(serde_json::to_vec(&library)?);
        hasher.update(serde_json::to_vec(&deployments)?);
        // Include live Library hashes even for unmatched candidates: a target selected later
        // must still be the same snapshot that the user reviewed.
        for skill in &library {
            if scope.is_some_and(|(_, target)| target != skill.id) {
                continue;
            }
            let path = library_root.join(&skill.directory);
            let canonical = path.canonicalize().ok();
            let hash = live_hashes
                .entry(canonical.clone().unwrap_or(path))
                .or_insert_with_key(|path| Acquisition::compute_library_hash(path).ok());
            let unchanged = if skill.content_hash.starts_with("v2:") {
                hash.as_deref() == Some(skill.content_hash.as_str())
            } else {
                Acquisition::hash_matches_baseline(
                    &library_root.join(&skill.directory),
                    &skill.content_hash,
                )
                .unwrap_or(false)
            };
            baseline_matches.insert(skill.id.clone(), unchanged);
            library_paths.insert(skill.id.clone(), canonical);
            hasher.update(serde_json::to_vec(&hash)?);
        }
        for (directory, entry) in lock.skills {
            if scope.is_some_and(|(candidate, _)| candidate != directory) {
                continue;
            }
            if !one_component(&directory) {
                warnings.push(format!("Ignored unsafe Skill directory: {directory}"));
                continue;
            }
            let path = root.join("skills").join(&directory);
            if !path.exists() {
                if fs::symlink_metadata(&path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    warnings.push(format!("{directory}: CLI symbolic link is broken; repair its deployment from the Skill Library"));
                }
                continue;
            }
            let lock_observation = serde_json::to_vec(&entry)?;
            let mut source = match lock_source(entry) {
                Ok(s) => s,
                Err(e) => {
                    warnings.push(format!("{directory}: {e}"));
                    continue;
                }
            };
            // A live deployment symlink is useful evidence too. Validate its resolved snapshot.
            let actual = path.canonicalize()?;
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                let linked = library.iter().find(|skill| {
                    // A scoped action may inspect a link associated with a
                    // different Library identity. Resolve metadata lazily for
                    // provenance checking without hashing those other Skills.
                    library_paths
                        .entry(skill.id.clone())
                        .or_insert_with(|| library_root.join(&skill.directory).canonicalize().ok())
                        .as_ref()
                        == Some(&actual)
                });
                let conflict = match linked {
                    Some(linked) => {
                        library.iter().any(|skill| {
                            same_origin(&skill.source, &source) && skill.id != linked.id
                        }) || (!same_origin(&linked.source, &source)
                            && !matches!(
                                linked.source.kind,
                                LibrarySourceKind::LocalImport | LibrarySourceKind::Zip
                            ))
                    }
                    None => true,
                };
                if conflict {
                    warnings.push(format!("{directory}: live symbolic link conflicts with lock provenance; associate its Library source manually"));
                    continue;
                }
            }
            let metadata = match source_inspections.entry(actual.clone()).or_insert_with(|| {
                Acquisition::inspect_source_directory(&actual).map_err(|e| e.to_string())
            }) {
                Ok(m) => m,
                Err(e) => {
                    warnings.push(format!("{directory}: {e}"));
                    continue;
                }
            };
            let matches: Vec<_> = library
                .iter()
                .filter(|s| same_origin(&s.source, &source))
                .collect();
            let matched = if matches.len() == 1 {
                Some(matches[0])
            } else {
                None
            };
            if source.repo_branch.is_none() {
                source.repo_branch = matched.and_then(|skill| skill.source.repo_branch.clone());
            }
            let suggested = library
                .iter()
                .filter(|s| {
                    s.directory == directory
                        || s.display_name.eq_ignore_ascii_case(&metadata.display_name)
                })
                .map(|s| s.id.clone())
                .collect();
            let live_hash = matched.and_then(|s| {
                let path = library_paths
                    .get(&s.id)
                    .and_then(Option::as_ref)
                    .cloned()
                    .unwrap_or_else(|| library_root.join(&s.directory));
                live_hashes.get(&path).and_then(Option::as_deref)
            });
            let source_matches_baseline = matched.is_some_and(|skill| {
                if skill.content_hash.starts_with("v2:") {
                    skill.content_hash == metadata.content_hash
                } else {
                    Acquisition::hash_matches_baseline(&actual, &skill.content_hash)
                        .unwrap_or(false)
                }
            });
            let replaced = !fs::symlink_metadata(&path)?.file_type().is_symlink()
                && deployments.iter().any(|d| {
                    d.library_directory == directory
                        && d.target
                            == super::skill_deployment::DeploymentTarget::global(
                                super::skill_deployment::DeploymentConsumer::Codex,
                            )
                });
            let mut candidate_hasher = Sha256::new();
            candidate_hasher.update(b"external-skill-target-v1");
            candidate_hasher.update(serde_json::to_vec(&(
                &directory,
                &lock_observation,
                &source,
                &metadata.content_hash,
                &path,
                &actual,
                fs::read_link(&path).ok(),
            ))?);
            // Bind physical source identity as well as its bytes. A replacement
            // with identical content still requires a fresh user observation.
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let entry_metadata = fs::symlink_metadata(&path)?;
                candidate_hasher.update(entry_metadata.dev().to_le_bytes());
                candidate_hasher.update(entry_metadata.ino().to_le_bytes());
            }
            let mut target_observation_tokens = BTreeMap::new();
            for skill in &library {
                if scope.is_some_and(|(_, target)| target != skill.id) {
                    continue;
                }
                let mut target_hasher = candidate_hasher.clone();
                let library_path = library_paths
                    .get(&skill.id)
                    .and_then(Option::as_ref)
                    .cloned()
                    .unwrap_or_else(|| library_root.join(&skill.directory));
                let relevant_deployments: Vec<_> = deployments
                    .iter()
                    .filter(|deployment| {
                        deployment.library_skill_id == skill.id
                            || (deployment.library_directory == directory
                                && deployment.target
                                    == super::skill_deployment::DeploymentTarget::global(
                                        super::skill_deployment::DeploymentConsumer::Codex,
                                    ))
                    })
                    .collect();
                target_hasher.update(serde_json::to_vec(&(
                    skill,
                    &library_path,
                    live_hashes.get(&library_path),
                    relevant_deployments,
                ))?);
                target_observation_tokens.insert(
                    skill.id.clone(),
                    format!("target-v1:{:x}", target_hasher.finalize()),
                );
            }
            let fully_synced = matched.is_some_and(|skill| {
                use super::skill_deployment::{DeploymentConsumer, DeploymentTarget};
                let target = DeploymentTarget::global(DeploymentConsumer::Codex);
                skill.directory == directory
                    && skill.source.repo_branch == source.repo_branch
                    && source_matches_baseline
                    && live_hash == Some(metadata.content_hash.as_str())
                    && metadata.compatibility.codex.compatible
                    && fs::read_link(&path).is_ok()
                    && library_paths.get(&skill.id).and_then(Option::as_ref) == Some(&actual)
                    && deployments.iter().any(|deployment| {
                        deployment.target == target
                            && deployment.library_skill_id == skill.id
                            && deployment.library_directory == directory
                    })
                    && !deployments.iter().any(|deployment| {
                        deployment.target == target
                            && deployment.library_directory == directory
                            && deployment.library_skill_id != skill.id
                    })
            });
            candidates.push(ExternalSkillCandidate {
                id: directory.clone(),
                directory,
                source,
                content_hash: metadata.content_hash.clone(),
                library_skill_id: matched.map(|s| s.id.clone()),
                suggested_library_skill_ids: suggested,
                changed: matched
                    .map(|_| {
                        !source_matches_baseline
                            || live_hash != Some(metadata.content_hash.as_str())
                    })
                    .unwrap_or(true),
                local_modified: matched
                    .map(|s| !baseline_matches.get(&s.id).copied().unwrap_or(false))
                    .unwrap_or(false),
                deployment_replaced: replaced,
                fully_synced,
                target_observation_tokens,
            });
            hasher.update(actual.to_string_lossy().as_bytes());
        }
        hasher.update(serde_json::to_vec(&candidates)?);
        Ok(ExternalSkillInspection {
            observation_token: format!("{:x}", hasher.finalize()),
            candidates,
            warnings,
        })
    }
    fn selected(
        db: &Arc<Database>,
        root: &Path,
        intent: &ExternalSkillIntent,
    ) -> Result<(ExternalSkillCandidate, LibrarySkill)> {
        let inspection = Self::inspect_for_observation(
            db,
            root,
            Some(&intent.candidate_id),
            &intent.library_skill_id,
            &intent.observation_token,
        )?;
        let global_matches = inspection.observation_token == intent.observation_token;
        let candidate = inspection
            .candidates
            .into_iter()
            .find(|c| c.id == intent.candidate_id)
            .ok_or_else(|| anyhow!("External Skill no longer exists"))?;
        if !global_matches
            && candidate
                .target_observation_tokens
                .get(&intent.library_skill_id)
                != Some(&intent.observation_token)
        {
            return Err(anyhow!("External Skill observation changed; inspect again"));
        }
        let skill = db
            .get_library_skill_by_id(&intent.library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found"))?;
        Ok((candidate, skill))
    }
    pub fn link_external(db: &Arc<Database>, intent: ExternalSkillIntent) -> Result<LibrarySkill> {
        let _guard = Acquisition::lock_for_composite()?;
        let (candidate, mut skill) = Self::selected(db, &Self::root(), &intent)?;
        skill.source = candidate.source;
        skill.updated_at = chrono::Utc::now().timestamp();
        db.update_library_skill_snapshot(&skill)?
            .ok_or_else(|| anyhow!("Library Skill disappeared"))
    }
    pub async fn link_source(
        db: &Arc<Database>,
        mut intent: LinkLibrarySkillSourceIntent,
    ) -> Result<LibrarySkill> {
        let observed = db
            .get_library_skill_by_id(&intent.library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found"))?;
        if observed.content_hash != intent.expected_content_hash {
            return Err(anyhow!("Library version changed; inspect again"));
        }
        intent.source = normalize_source(intent.source)?;
        resolve_branch(&mut intent.source).await?;
        let (_snapshot, root) =
            Acquisition::download_repository_snapshot_exact(&intent.source).await?;
        let candidate = root.join(intent.source.skill_path.as_deref().unwrap_or("."));
        if !candidate.canonicalize()?.starts_with(root.canonicalize()?) {
            return Err(anyhow!("Skill path escapes repository snapshot"));
        }
        Acquisition::inspect_source_directory(&candidate)?;
        let _guard = Acquisition::lock_for_composite()?;
        let mut skill = db
            .get_library_skill_by_id(&intent.library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found"))?;
        if skill != observed {
            return Err(anyhow!("Library metadata changed; inspect again"));
        }
        skill.source = intent.source;
        skill.updated_at = chrono::Utc::now().timestamp();
        db.update_library_skill_snapshot(&skill)?
            .ok_or_else(|| anyhow!("Library Skill disappeared"))
    }
    pub fn apply(
        db: &Arc<Database>,
        intent: ExternalSkillIntent,
    ) -> Result<LibrarySkillUpdateResult> {
        let root = Self::root();
        let (candidate, skill) = Self::selected(db, &root, &intent)?;
        let deployment_was_desired = if intent.restore_deployment {
            match Self::preflight_deployment(db, &candidate, &skill) {
                Ok(was_desired) => was_desired,
                Err(error) => {
                    return Ok(LibrarySkillUpdateResult {
                        outcome: LibrarySkillUpdateApplyOutcome::Blocked,
                        library_skill_id: skill.id.clone(),
                        reason: Some(LibrarySkillUpdateReason::InvalidCandidate),
                        recorded_content_hash: Some(skill.content_hash.clone()),
                        live_content_hash: None,
                        staged_content_hash: None,
                        affected_deployments: Vec::new(),
                        backup_path: None,
                        message: Some(error.to_string()),
                    })
                }
            }
        } else {
            false
        };
        let path = root
            .join("skills")
            .join(&candidate.directory)
            .canonicalize()?;
        let check = LibrarySkillUpdateService::stage_external(
            db,
            &skill,
            &path,
            candidate.source,
            &root,
            &intent.observation_token,
            &intent.candidate_id,
        )?;
        let stage_token = check.stage_token.clone().unwrap();
        let applied = LibrarySkillUpdateService::apply(
            db,
            LibrarySkillUpdateApplyIntent {
                library_skill_id: skill.id.clone(),
                stage_token: check.stage_token.unwrap(),
                observation_token: check.observation_token,
                confirm_local_modifications: intent.confirm_local_modifications,
            },
        );
        // External apply does not expose a reusable stage token. Retain only recovery
        // evidence, never abandoned snapshots from blocked attempts.
        let cleanup_error = if !matches!(&applied, Ok(result) if result.outcome == LibrarySkillUpdateApplyOutcome::RecoveryRequired)
        {
            LibrarySkillUpdateService::discard_external_stage(&stage_token).err()
        } else {
            None
        };
        let mut result = applied.map_err(|error| match &cleanup_error {
            Some(cleanup) => anyhow!("{error}; external stage cleanup also failed: {cleanup}"),
            None => error,
        })?;
        if (candidate.deployment_replaced || intent.restore_deployment)
            && matches!(
                result.outcome,
                LibrarySkillUpdateApplyOutcome::Updated | LibrarySkillUpdateApplyOutcome::UpToDate
            )
        {
            if intent.restore_deployment {
                if let Err(e) = Self::restore(
                    db,
                    &candidate.directory,
                    &candidate.content_hash,
                    &skill.id,
                    deployment_was_desired,
                ) {
                    result.outcome = LibrarySkillUpdateApplyOutcome::RecoveryRequired;
                    result.message = Some(format!(
                        "Library updated; CLI deployment needs attention: {e}"
                    ));
                }
            } else {
                result.message=Some("Library updated; external directory remains a deployment conflict until explicitly restored".into());
            }
        }
        if let Some(error) = cleanup_error {
            let previous = result.message.take().unwrap_or_default();
            result.message = Some(format!("{previous} External stage cleanup failed: {error}"));
        }
        Ok(result)
    }
    fn preflight_deployment(
        db: &Arc<Database>,
        candidate: &ExternalSkillCandidate,
        skill: &LibrarySkill,
    ) -> Result<bool> {
        use super::skill_deployment::{DeploymentConsumer, DeploymentTarget};
        if candidate.directory != skill.directory {
            return Err(anyhow!(
                "Cannot replace CLI directory '{}' with Library Skill '{}': deployment directories must match the Library's immutable directory name. Choose a matching Library Skill or use Link only.",
                candidate.directory,
                skill.directory
            ));
        }
        let target = DeploymentTarget::global(DeploymentConsumer::Codex);
        let deployments = db.list_skill_deployments()?;
        if deployments.iter().any(|deployment| {
            deployment.target == target
                && deployment.library_directory == candidate.directory
                && deployment.library_skill_id != skill.id
        }) {
            return Err(anyhow!("CLI directory is desired by another Library Skill"));
        }
        let was_desired = deployments.iter().any(|deployment| {
            deployment.target == target
                && deployment.library_skill_id == skill.id
                && deployment.library_directory == candidate.directory
        });
        let path = Self::root().join("skills").join(&candidate.directory);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            if path.canonicalize()?
                != Acquisition::library_directory_path()
                    .join(&skill.directory)
                    .canonicalize()?
            {
                return Err(anyhow!("CLI symbolic link points to another Library Skill"));
            }
            if !Acquisition::inspect_source_directory(&path.canonicalize()?)?
                .compatibility
                .codex
                .compatible
            {
                return Err(anyhow!("CLI Library link is not compatible with Codex"));
            }
            if !was_desired {
                use super::deployment_recovery::{
                    DeploymentRecoveryDisposition, DeploymentRecoveryService,
                };
                let finding = DeploymentRecoveryService::new(db.clone())
                    .inspect_candidate(&skill.id, &target)?;
                if finding.disposition != DeploymentRecoveryDisposition::Recoverable {
                    return Err(anyhow!(
                        "CLI link cannot be adopted safely: {:?}",
                        finding.disposition
                    ));
                }
            }
            return Ok(was_desired);
        }
        let inspection = super::skill_import::GlobalSkillImportService::new(db.clone())
            .inspect_external(&candidate.directory)?;
        let finding = inspection
            .findings
            .iter()
            .find(|finding| {
                finding.directory == candidate.directory
                    && finding.consumer == DeploymentConsumer::Codex
            })
            .ok_or_else(|| anyhow!("CLI directory is not eligible for import and replacement"))?;
        if !finding.replace_eligibility.eligible {
            return Err(anyhow!(
                "CLI directory cannot be replaced: {:?}",
                finding.replace_eligibility.reason
            ));
        }
        Ok(was_desired)
    }

    fn restore(
        db: &Arc<Database>,
        directory: &str,
        hash: &str,
        id: &str,
        was_desired: bool,
    ) -> Result<()> {
        use super::skill_import::{
            GlobalSkillImportIntent, GlobalSkillImportMode, GlobalSkillImportResolution,
            GlobalSkillImportService, ProjectSkillImportOutcome,
        };
        let path = Self::root().join("skills").join(directory);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            use super::deployment_recovery::DeploymentRecoveryService;
            use super::skill_deployment::{
                DeploymentConsumer, DeploymentIntent, DeploymentMutationOutcome, DeploymentTarget,
                SkillDeploymentService,
            };
            let _library_guard = Acquisition::lock_for_composite()?;
            let _deployment_guard = SkillDeploymentService::lock_for_composite()?;
            let target = DeploymentTarget::global(DeploymentConsumer::Codex);
            if db.get_skill_deployment(id, &target)?.is_some() != was_desired {
                return Err(anyhow!("Desired deployment changed before CLI replacement"));
            }
            if path.canonicalize()?
                == Acquisition::library_directory_path()
                    .join(directory)
                    .canonicalize()?
            {
                let service = SkillDeploymentService::new(db.clone());
                let deploy_intent = if was_desired {
                    DeploymentIntent::Deploy {
                        library_skill_id: id.into(),
                        target,
                    }
                } else {
                    let finding = DeploymentRecoveryService::new(db.clone())
                        .inspect_candidate(id, &target)?;
                    DeploymentIntent::Recover {
                        library_skill_id: id.into(),
                        target,
                        observation_token: finding
                            .observation_token
                            .ok_or_else(|| anyhow!("CLI link cannot be adopted safely"))?,
                        confirmed: true,
                    }
                };
                let deployed = service.apply_one_for_composite(&deploy_intent);
                service.record_composite_activity(&deploy_intent, &deployed);
                let deployed = deployed?;
                if matches!(
                    deployed.outcome,
                    DeploymentMutationOutcome::Applied | DeploymentMutationOutcome::AlreadyInSync
                ) {
                    return Ok(());
                }
                return Err(anyhow!("{:?}: {:?}", deployed.outcome, deployed.message));
            }
            return Err(anyhow!("CLI symbolic link changed before deployment"));
        }
        if Acquisition::compute_library_hash(&path)? != hash {
            return Err(anyhow!("External content changed before restoration"));
        }
        let service = GlobalSkillImportService::new(db.clone());
        let inspection = service.inspect_external(directory)?;
        let finding = inspection
            .findings
            .into_iter()
            .find(|f| {
                f.directory == directory
                    && f.consumer == super::skill_deployment::DeploymentConsumer::Codex
            })
            .ok_or_else(|| anyhow!("External directory cannot be restored"))?;
        let result = service.replace_external(
            GlobalSkillImportIntent {
                finding_id: finding.id,
                observation_token: inspection.observation_token,
                mode: GlobalSkillImportMode::ImportAndReplace,
                resolution: GlobalSkillImportResolution::Reuse {
                    library_skill_id: id.into(),
                },
            },
            id,
            directory,
            was_desired,
        )?;
        if result.outcome != ProjectSkillImportOutcome::Deployed {
            return Err(anyhow!(
                "{:?}: {:?}; CLI directory backup: {}",
                result.outcome,
                result.message,
                result.backup_path.as_deref().unwrap_or("not created")
            ));
        }
        Ok(())
    }
}
fn one_component(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\\')
        && Path::new(s).components().count() == 1
        && matches!(Path::new(s).components().next(), Some(Component::Normal(_)))
}
fn same_origin(a: &LibrarySkillSource, b: &LibrarySkillSource) -> bool {
    matches!(
        a.kind,
        LibrarySourceKind::Git | LibrarySourceKind::Marketplace
    ) && a
        .repo_owner
        .as_deref()
        .zip(b.repo_owner.as_deref())
        .map(|(a, b)| a.eq_ignore_ascii_case(b))
        .unwrap_or(false)
        && a.repo_name
            .as_deref()
            .zip(b.repo_name.as_deref())
            .map(|(a, b)| a.eq_ignore_ascii_case(b))
            .unwrap_or(false)
        && a.skill_path.as_deref().unwrap_or(".") == b.skill_path.as_deref().unwrap_or(".")
}
fn normalize_source(mut source: LibrarySkillSource) -> Result<LibrarySkillSource> {
    if !matches!(
        source.kind,
        LibrarySourceKind::Git | LibrarySourceKind::Marketplace
    ) {
        return Err(anyhow!("Expected GitHub upstream source"));
    }
    let owner = source
        .repo_owner
        .as_deref()
        .ok_or_else(|| anyhow!("Repository owner required"))?;
    let repo = source
        .repo_name
        .as_deref()
        .ok_or_else(|| anyhow!("Repository name required"))?;
    SkillService::validate_repo_ref(owner, repo, source.repo_branch.as_deref().unwrap_or("main"))?;
    let path = source
        .skill_path
        .as_deref()
        .ok_or_else(|| anyhow!("Explicit repository Skill path required"))?;
    if path != "."
        && (path.is_empty()
            || path.contains('\\')
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_))))
    {
        return Err(anyhow!("Skill path must stay inside repository"));
    }
    source.url = Some(format!("https://github.com/{owner}/{repo}"));
    Ok(source)
}
fn lock_source(entry: LockSkill) -> Result<LibrarySkillSource> {
    if entry.source_type.as_deref() != Some("github") {
        return Err(anyhow!("No supported GitHub source in lock file"));
    }
    let repo = entry
        .source
        .ok_or_else(|| anyhow!("Missing source coordinates"))?;
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| anyhow!("Invalid source coordinates"))?;
    let mut path = entry
        .skill_path
        .ok_or_else(|| anyhow!("Missing upstream Skill path; associate manually"))?;
    if path == "SKILL.md" {
        path = ".".into();
    } else if let Some(parent) = path.strip_suffix("/SKILL.md") {
        path = parent.to_string();
    }
    let branch = entry.branch.or(entry.source_branch).or_else(|| {
        entry.source_url.as_deref().and_then(|url| {
            url.split_once("/tree/")
                .map(|(_, rest)| rest.split('/').next().unwrap_or("").to_string())
        })
    });
    normalize_source(LibrarySkillSource {
        kind: LibrarySourceKind::Git,
        url: None,
        repo_owner: Some(owner.into()),
        repo_name: Some(name.trim_end_matches(".git").into()),
        repo_branch: branch,
        skill_path: Some(path),
        marketplace: None,
    })
}
pub(crate) async fn resolve_branch(source: &mut LibrarySkillSource) -> Result<()> {
    if source.repo_branch.is_none() {
        let url = format!(
            "https://api.github.com/repos/{}/{}",
            source.repo_owner.as_deref().unwrap_or_default(),
            source.repo_name.as_deref().unwrap_or_default()
        );
        let body: serde_json::Value = crate::proxy::http_client::get()
            .get(url)
            .header("User-Agent", "cc-switch-plus")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        source.repo_branch = Some(
            body.get("default_branch")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("GitHub did not return a default branch"))?
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lock(path: &str) -> LockSkill {
        serde_json::from_value(
            serde_json::json!({"source":"Example/Skills","sourceType":"github","skillPath":path}),
        )
        .unwrap()
    }
    #[test]
    fn repository_and_path_define_origin_not_name_or_version() {
        let a = lock_source(lock("skills/implementation/SKILL.md")).unwrap();
        let mut b = a.clone();
        b.repo_owner = Some("example".into());
        b.repo_name = Some("skills".into());
        b.repo_branch = Some("release".into());
        assert!(same_origin(&a, &b));
        b.skill_path = Some("different/implementation".into());
        assert!(!same_origin(&a, &b));
        b = a.clone();
        b.kind = LibrarySourceKind::LocalImport;
        assert!(!same_origin(&b, &a));
    }
    #[test]
    fn lock_paths_normalize_manifest_and_reject_escape() {
        assert_eq!(
            lock_source(lock("SKILL.md")).unwrap().skill_path.as_deref(),
            Some(".")
        );
        for path in [
            "../secret/SKILL.md",
            "/etc/SKILL.md",
            "skills/../secret/SKILL.md",
            "skills\\secret/SKILL.md",
        ] {
            assert!(lock_source(lock(path)).is_err(), "{path}");
        }
        assert!(!one_component("../implementation"));
        assert!(!one_component("."));
        assert!(one_component("implementation"));
    }
}
