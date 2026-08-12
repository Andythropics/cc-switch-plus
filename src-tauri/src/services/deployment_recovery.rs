//! Read-only discovery of exact, unrecorded links into the private Skill Library.
//!
//! Callers select only typed Deployment scopes. Filesystem paths are resolved
//! internally and returned for display only; adoption is performed by the
//! normal Deployment `apply` seam after a fresh observation check.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::get_home_dir;
use crate::database::Database;
use crate::services::project_workspace::{
    consumer_directory, project_workspace_lifecycle, workspace_root_matches_identity,
    ProjectWorkspace, WorkspaceLifecycle,
};
use crate::services::skill::{LibrarySkill, LibrarySkillCompatibility};
use crate::services::skill_deployment::{DeploymentConsumer, DeploymentTarget, WorkspaceKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecoveryQuery {
    #[serde(default)]
    pub consumer: Option<DeploymentConsumer>,
    #[serde(default)]
    pub workspace: Option<WorkspaceKind>,
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentRecoveryDisposition {
    Recoverable,
    DesiredExists,
    ForeignLink,
    AmbiguousLink,
    BrokenLink,
    EscapingLink,
    NonLibrary,
    Occupied,
    Unreadable,
    InvalidRoot,
    Incompatible,
    ArchivedWorkspace,
    UnavailableWorkspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentRecoveryReason {
    ExactLibraryLink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecoveryFinding {
    pub disposition: DeploymentRecoveryDisposition,
    pub target: DeploymentTarget,
    pub entry_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_reason: Option<DeploymentRecoveryReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecoveryInspectionResult {
    pub findings: Vec<DeploymentRecoveryFinding>,
}

pub struct DeploymentRecoveryService {
    db: Arc<Database>,
}

impl DeploymentRecoveryService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn inspect(
        &self,
        query: DeploymentRecoveryQuery,
    ) -> Result<DeploymentRecoveryInspectionResult> {
        Self::validate_query(&query)?;
        let skills = self.db.list_library_skills()?;
        let desired = self.db.list_skill_deployments()?;
        let targets = self.targets(&query)?;
        let mut findings = Vec::new();
        for scope in targets {
            let lifecycle = scope
                .workspace
                .as_ref()
                .map(|workspace| project_workspace_lifecycle(&self.db, &workspace.id))
                .transpose()?;
            if lifecycle == Some(WorkspaceLifecycle::Unavailable)
                || scope.workspace.as_ref().is_some_and(|workspace| {
                    !workspace_root_matches_identity(workspace).unwrap_or(false)
                })
            {
                findings.push(Self::scope_finding(
                    scope.target,
                    DeploymentRecoveryDisposition::UnavailableWorkspace,
                ));
                continue;
            }
            findings.extend(self.inspect_root(
                &scope.target,
                &scope.root,
                lifecycle,
                &skills,
                &desired,
            )?);
        }
        findings.sort_by(|left, right| {
            Self::target_sort_key(&left.target)
                .cmp(&Self::target_sort_key(&right.target))
                .then(left.entry_name.cmp(&right.entry_name))
                .then(left.library_skill_id.cmp(&right.library_skill_id))
        });
        Ok(DeploymentRecoveryInspectionResult { findings })
    }

    pub(crate) fn inspect_candidate(
        &self,
        library_skill_id: &str,
        target: &DeploymentTarget,
    ) -> Result<DeploymentRecoveryFinding> {
        let skill = self
            .db
            .get_library_skill_by_id(library_skill_id)?
            .ok_or_else(|| anyhow!("Library Skill not found: {library_skill_id}"))?;
        let query = DeploymentRecoveryQuery {
            consumer: Some(target.consumer),
            workspace: Some(target.workspace),
            workspace_id: (target.workspace == WorkspaceKind::Project)
                .then(|| target.workspace_id.clone()),
        };
        let findings = self.inspect(query)?.findings;
        let exact = findings.iter().find(|finding| {
            finding.target == *target
                && (finding.library_skill_id.as_deref() == Some(library_skill_id)
                    || finding.entry_name == skill.directory)
        });
        if let Some(exact) = exact {
            return Ok(exact.clone());
        }
        let scope = findings
            .into_iter()
            .find(|finding| finding.target == *target && finding.entry_name.is_empty());
        Ok(scope.unwrap_or_else(|| DeploymentRecoveryFinding {
            disposition: DeploymentRecoveryDisposition::BrokenLink,
            target: target.clone(),
            entry_name: skill.directory.clone(),
            library_skill_id: Some(skill.id),
            library_directory: Some(skill.directory),
            observed_target: None,
            observation_token: None,
            safe_reason: None,
        }))
    }

    fn validate_query(query: &DeploymentRecoveryQuery) -> Result<()> {
        match (query.workspace, query.workspace_id.as_deref()) {
            (Some(WorkspaceKind::Global), Some(id)) if !id.is_empty() => Err(anyhow!(
                "global recovery inspection cannot carry a Workspace identity"
            )),
            (Some(WorkspaceKind::Project), Some(id)) if id.trim().is_empty() => Err(anyhow!(
                "project recovery Workspace identity cannot be empty"
            )),
            (None, Some(_)) => Err(anyhow!(
                "recovery workspaceId requires workspace to be project"
            )),
            _ => Ok(()),
        }
    }

    fn targets(&self, query: &DeploymentRecoveryQuery) -> Result<Vec<RecoveryScope>> {
        let consumers: Vec<_> = match query.consumer {
            Some(consumer) => vec![consumer],
            None => vec![DeploymentConsumer::Claude, DeploymentConsumer::Codex],
        };
        let mut scopes = Vec::new();
        if query.workspace != Some(WorkspaceKind::Project) {
            for consumer in &consumers {
                scopes.push(RecoveryScope {
                    target: DeploymentTarget::global(*consumer),
                    root: global_root(*consumer),
                    workspace: None,
                });
            }
        }
        if query.workspace != Some(WorkspaceKind::Global) {
            for workspace in self.db.list_project_workspaces()? {
                if query
                    .workspace_id
                    .as_ref()
                    .is_some_and(|id| id != &workspace.id)
                {
                    continue;
                }
                for consumer in &consumers {
                    scopes.push(RecoveryScope {
                        target: DeploymentTarget {
                            consumer: *consumer,
                            workspace: WorkspaceKind::Project,
                            workspace_id: workspace.id.clone(),
                        },
                        root: workspace
                            .root_path
                            .join(consumer_directory(*consumer))
                            .join("skills"),
                        workspace: Some(workspace.clone()),
                    });
                }
            }
        }
        Ok(scopes)
    }

    fn inspect_root(
        &self,
        target: &DeploymentTarget,
        root: &Path,
        lifecycle: Option<WorkspaceLifecycle>,
        skills: &[LibrarySkill],
        desired: &[crate::services::skill_deployment::DesiredDeployment],
    ) -> Result<Vec<DeploymentRecoveryFinding>> {
        match fs::symlink_metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => {
                return Ok(vec![Self::scope_finding(
                    target.clone(),
                    DeploymentRecoveryDisposition::Unreadable,
                )])
            }
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Ok(vec![Self::scope_finding(
                    target.clone(),
                    DeploymentRecoveryDisposition::InvalidRoot,
                )])
            }
            Ok(_) => {}
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => {
                return Ok(vec![Self::scope_finding(
                    target.clone(),
                    DeploymentRecoveryDisposition::Unreadable,
                )])
            }
        };
        let mut findings = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => findings.push(self.inspect_entry(
                    target,
                    &entry.path(),
                    lifecycle,
                    skills,
                    desired,
                )),
                Err(_) => findings.push(Self::scope_finding(
                    target.clone(),
                    DeploymentRecoveryDisposition::Unreadable,
                )),
            }
        }
        Ok(findings)
    }

    fn inspect_entry(
        &self,
        target: &DeploymentTarget,
        entry: &Path,
        lifecycle: Option<WorkspaceLifecycle>,
        skills: &[LibrarySkill],
        desired: &[crate::services::skill_deployment::DesiredDeployment],
    ) -> DeploymentRecoveryFinding {
        let entry_name = entry
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let base = |disposition, observed_target| DeploymentRecoveryFinding {
            disposition,
            target: target.clone(),
            entry_name: entry_name.clone(),
            library_skill_id: None,
            library_directory: None,
            observed_target,
            observation_token: None,
            safe_reason: None,
        };
        let metadata = match fs::symlink_metadata(entry) {
            Ok(metadata) => metadata,
            Err(_) => return base(DeploymentRecoveryDisposition::Unreadable, None),
        };
        if !metadata.file_type().is_symlink() {
            return base(DeploymentRecoveryDisposition::Occupied, None);
        }
        let raw = match fs::read_link(entry) {
            Ok(raw) => raw,
            Err(_) => return base(DeploymentRecoveryDisposition::Unreadable, None),
        };
        let raw_display = Some(raw.display().to_string());
        if !raw.is_absolute() {
            return base(DeploymentRecoveryDisposition::AmbiguousLink, raw_display);
        }
        let library_root = crate::config::get_app_config_dir().join("skills");
        let canonical_library_root = fs::canonicalize(&library_root).ok();
        let canonical_actual = match fs::canonicalize(&raw) {
            Ok(path) => path,
            Err(_) => return base(DeploymentRecoveryDisposition::BrokenLink, raw_display),
        };
        let skill = skills.iter().find(|skill| skill.directory == entry_name);
        let Some(skill) = skill else {
            let matches_other_library_skill = skills.iter().any(|skill| {
                let expected = library_root.join(&skill.directory);
                raw == expected
                    && fs::canonicalize(expected)
                        .ok()
                        .as_ref()
                        .is_some_and(|expected| expected == &canonical_actual)
            });
            let disposition = if matches_other_library_skill {
                DeploymentRecoveryDisposition::AmbiguousLink
            } else if raw.starts_with(&library_root) {
                if canonical_library_root
                    .as_ref()
                    .is_some_and(|root| !canonical_actual.starts_with(root))
                {
                    DeploymentRecoveryDisposition::EscapingLink
                } else {
                    DeploymentRecoveryDisposition::NonLibrary
                }
            } else if canonical_library_root
                .as_ref()
                .is_some_and(|root| canonical_actual.starts_with(root))
            {
                DeploymentRecoveryDisposition::AmbiguousLink
            } else {
                DeploymentRecoveryDisposition::ForeignLink
            };
            return base(disposition, raw_display);
        };
        let expected = library_root.join(&skill.directory);
        let canonical_expected = fs::canonicalize(&expected).ok();
        let mut finding = DeploymentRecoveryFinding {
            disposition: DeploymentRecoveryDisposition::AmbiguousLink,
            target: target.clone(),
            entry_name,
            library_skill_id: Some(skill.id.clone()),
            library_directory: Some(skill.directory.clone()),
            observed_target: raw_display,
            observation_token: None,
            safe_reason: None,
        };
        if raw != expected {
            finding.disposition = if raw.starts_with(&library_root) {
                if canonical_library_root
                    .as_ref()
                    .is_some_and(|root| !canonical_actual.starts_with(root))
                {
                    DeploymentRecoveryDisposition::EscapingLink
                } else {
                    DeploymentRecoveryDisposition::AmbiguousLink
                }
            } else if canonical_library_root
                .as_ref()
                .is_some_and(|root| canonical_actual.starts_with(root))
            {
                DeploymentRecoveryDisposition::AmbiguousLink
            } else {
                DeploymentRecoveryDisposition::ForeignLink
            };
            return finding;
        }
        if !expected.is_dir() {
            finding.disposition = DeploymentRecoveryDisposition::NonLibrary;
            return finding;
        }
        if fs::symlink_metadata(&expected).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            finding.disposition = if canonical_library_root
                .as_ref()
                .is_some_and(|root| canonical_actual.starts_with(root))
            {
                DeploymentRecoveryDisposition::AmbiguousLink
            } else {
                DeploymentRecoveryDisposition::EscapingLink
            };
            return finding;
        }
        if canonical_expected.as_ref() != Some(&canonical_actual)
            || canonical_library_root
                .as_ref()
                .is_none_or(|root| !canonical_actual.starts_with(root))
        {
            finding.disposition = DeploymentRecoveryDisposition::EscapingLink;
            return finding;
        }
        if desired
            .iter()
            .any(|row| row.library_skill_id == skill.id && row.target == *target)
        {
            finding.disposition = DeploymentRecoveryDisposition::DesiredExists;
            return finding;
        }
        if lifecycle == Some(WorkspaceLifecycle::Archived) {
            finding.disposition = DeploymentRecoveryDisposition::ArchivedWorkspace;
            return finding;
        }
        if !compatible(&skill.compatibility, target.consumer) {
            finding.disposition = DeploymentRecoveryDisposition::Incompatible;
            return finding;
        }
        let token = observation_token(skill, target, &raw, &canonical_actual, &metadata);
        finding.disposition = DeploymentRecoveryDisposition::Recoverable;
        finding.observation_token = Some(token);
        finding.safe_reason = Some(DeploymentRecoveryReason::ExactLibraryLink);
        finding
    }

    fn scope_finding(
        target: DeploymentTarget,
        disposition: DeploymentRecoveryDisposition,
    ) -> DeploymentRecoveryFinding {
        DeploymentRecoveryFinding {
            disposition,
            target,
            entry_name: String::new(),
            library_skill_id: None,
            library_directory: None,
            observed_target: None,
            observation_token: None,
            safe_reason: None,
        }
    }

    fn target_sort_key(target: &DeploymentTarget) -> (&'static str, &str, &'static str) {
        (
            match target.workspace {
                WorkspaceKind::Global => "global",
                WorkspaceKind::Project => "project",
            },
            target.workspace_id.as_str(),
            match target.consumer {
                DeploymentConsumer::Claude => "claude",
                DeploymentConsumer::Codex => "codex",
            },
        )
    }
}

struct RecoveryScope {
    target: DeploymentTarget,
    root: PathBuf,
    workspace: Option<ProjectWorkspace>,
}

fn global_root(consumer: DeploymentConsumer) -> PathBuf {
    match consumer {
        DeploymentConsumer::Claude => get_home_dir().join(".claude/skills"),
        DeploymentConsumer::Codex => get_home_dir().join(".agents/skills"),
    }
}

fn compatible(compatibility: &LibrarySkillCompatibility, consumer: DeploymentConsumer) -> bool {
    match consumer {
        DeploymentConsumer::Claude => compatibility.claude.compatible,
        DeploymentConsumer::Codex => compatibility.codex.compatible,
    }
}

fn observation_token(
    skill: &LibrarySkill,
    target: &DeploymentTarget,
    raw: &Path,
    canonical: &Path,
    link_metadata: &fs::Metadata,
) -> String {
    let source_metadata = fs::metadata(canonical).ok();
    let mut hasher = Sha256::new();
    hasher.update(skill.id.as_bytes());
    hasher.update([0]);
    hasher.update(skill.directory.as_bytes());
    hasher.update([0]);
    hasher.update(skill.content_hash.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(target).unwrap_or_default());
    hasher.update([0]);
    hasher.update(raw.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(link_metadata.dev().to_le_bytes());
    hasher.update(link_metadata.ino().to_le_bytes());
    if let Some(metadata) = source_metadata {
        hasher.update(metadata.dev().to_le_bytes());
        hasher.update(metadata.ino().to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}
