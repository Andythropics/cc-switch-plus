//! Project Workspace registration and read-only inventory.
//!
//! Registration resolves a user-selected directory to one canonical physical
//! root, persists only the Workspace identity, and scans existing consumer
//! Skill scopes without creating or changing project files. Deployment owns
//! all later filesystem mutations through the shared inspect/apply seam.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::database::Database;
use crate::services::skill_deployment::DeploymentConsumer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRootKind {
    GitRepository,
    GitWorktree,
    NonGit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLifecycle {
    Active,
    Archived,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkspace {
    pub id: String,
    pub display_name: String,
    pub root_path: PathBuf,
    pub root_kind: WorkspaceRootKind,
    pub lifecycle: WorkspaceLifecycle,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceScopeKind {
    RootLevel,
    NestedUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSkillScope {
    pub consumer: DeploymentConsumer,
    pub path: PathBuf,
    pub kind: WorkspaceScopeKind,
    pub skill_directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRegistrationScan {
    pub selected_path: PathBuf,
    pub canonical_root: PathBuf,
    pub root_kind: WorkspaceRootKind,
    pub scopes: Vec<WorkspaceSkillScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRegistration {
    pub workspace: ProjectWorkspace,
    pub scan: WorkspaceRegistrationScan,
}

pub struct ProjectWorkspaceService {
    db: Arc<Database>,
}

impl ProjectWorkspaceService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Inspect a selected path without writing a database row or project file.
    pub fn inspect_path(&self, selected_path: &Path) -> Result<WorkspaceRegistrationScan> {
        Self::ensure_supported_platform()?;
        Self::scan_path(selected_path)
    }

    /// Resolve, scan, and persist one active Project Workspace identity.
    pub fn register(
        &self,
        selected_path: &Path,
        display_name: Option<String>,
    ) -> Result<WorkspaceRegistration> {
        let scan = Self::scan_path(selected_path)?;
        let root = &scan.canonical_root;
        for existing in self.db.list_project_workspaces()? {
            if paths_overlap(&existing.root_path, root) {
                return Err(anyhow!(
                    "Project Workspace overlaps registered root {}",
                    existing.root_path.display()
                ));
            }
        }
        let now = Utc::now().timestamp();
        let default_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Project Workspace")
            .to_string();
        let workspace = ProjectWorkspace {
            id: uuid::Uuid::new_v4().to_string(),
            display_name: display_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(default_name),
            root_path: root.clone(),
            root_kind: scan.root_kind,
            lifecycle: WorkspaceLifecycle::Active,
            created_at: now,
            updated_at: now,
        };
        self.db.save_project_workspace(&workspace)?;
        Ok(WorkspaceRegistration { workspace, scan })
    }

    pub fn list(&self) -> Result<Vec<ProjectWorkspace>> {
        Self::ensure_supported_platform()?;
        Ok(self
            .db
            .list_project_workspaces()?
            .into_iter()
            .map(|mut workspace| {
                if workspace.lifecycle == WorkspaceLifecycle::Active
                    && !workspace.root_path.is_dir()
                {
                    workspace.lifecycle = WorkspaceLifecycle::Unavailable;
                }
                workspace
            })
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<ProjectWorkspace>> {
        Self::ensure_supported_platform()?;
        let Some(mut workspace) = self.db.get_project_workspace(id)? else {
            return Ok(None);
        };
        if workspace.lifecycle == WorkspaceLifecycle::Active && !workspace.root_path.is_dir() {
            workspace.lifecycle = WorkspaceLifecycle::Unavailable;
        }
        Ok(Some(workspace))
    }

    fn scan_path(selected_path: &Path) -> Result<WorkspaceRegistrationScan> {
        let selected_path = fs::canonicalize(selected_path).with_context(|| {
            format!(
                "Project Workspace directory is not accessible: {}",
                selected_path.display()
            )
        })?;
        if !selected_path.is_dir() {
            return Err(anyhow!(
                "Project Workspace selection must be a directory: {}",
                selected_path.display()
            ));
        }
        let (canonical_root, root_kind) = resolve_project_root(&selected_path)?;
        let scopes = scan_consumer_scopes(&canonical_root)?;
        Ok(WorkspaceRegistrationScan {
            selected_path,
            canonical_root,
            root_kind,
            scopes,
        })
    }

    fn ensure_supported_platform() -> Result<()> {
        if !cfg!(target_os = "macos") {
            return Err(anyhow!(
                "the redesigned Project Workspace system is supported on macOS only"
            ));
        }
        Ok(())
    }
}

/// Resolve the target root for the shared Deployment service. Callers pass a
/// Workspace identity; no caller-provided filesystem path is accepted.
pub(crate) fn project_target_root(
    db: &Database,
    workspace_id: &str,
    consumer: DeploymentConsumer,
) -> Result<PathBuf> {
    let workspace = db
        .get_project_workspace(workspace_id)?
        .ok_or_else(|| anyhow!("Project Workspace not found: {workspace_id}"))?;
    if workspace.lifecycle != WorkspaceLifecycle::Active {
        return Err(anyhow!("Project Workspace is not active"));
    }
    if !workspace.root_path.is_dir() {
        return Err(anyhow!("Project Workspace is unavailable"));
    }
    Ok(workspace
        .root_path
        .join(consumer_directory(consumer))
        .join("skills"))
}

pub(crate) fn project_git_exclude_path(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.display().to_string(),
            "rev-parse",
            "--git-path",
            "info/exclude",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

pub(crate) fn consumer_directory(consumer: DeploymentConsumer) -> &'static str {
    match consumer {
        DeploymentConsumer::Claude => ".claude",
        DeploymentConsumer::Codex => ".agents",
    }
}

pub(crate) fn add_git_exclude(
    root: &Path,
    consumer: DeploymentConsumer,
    directory: &str,
) -> Result<Option<PathBuf>> {
    let Some(path) = project_git_exclude_path(root) else {
        return Ok(None);
    };
    let entry = git_exclude_entry(consumer, directory);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|line| line == entry) {
        return Ok(Some(path));
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&entry);
    updated.push('\n');
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow!("invalid Git exclude path"))?,
    )?;
    fs::write(&path, updated)?;
    Ok(Some(path))
}

pub(crate) fn remove_git_exclude(
    path: &Path,
    consumer: DeploymentConsumer,
    directory: &str,
) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let entry = git_exclude_entry(consumer, directory);
    let lines = existing
        .lines()
        .filter(|line| *line != entry)
        .collect::<Vec<_>>();
    if lines.len() == existing.lines().count() {
        return Ok(());
    }
    let mut output = lines.join("\n");
    if existing.ends_with('\n') && !output.is_empty() {
        output.push('\n');
    }
    fs::write(path, output)?;
    Ok(())
}

fn git_exclude_entry(consumer: DeploymentConsumer, directory: &str) -> String {
    format!(
        "/{}/{}/{} # cc-switch managed",
        consumer_directory(consumer),
        "skills",
        directory
    )
}

fn resolve_project_root(selected_path: &Path) -> Result<(PathBuf, WorkspaceRootKind)> {
    let output = Command::new("git")
        .args([
            "-C",
            &selected_path.display().to_string(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            let output_path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if output_path.is_dir() {
                let root = fs::canonicalize(output_path)?;
                let kind = if root.join(".git").is_file() {
                    WorkspaceRootKind::GitWorktree
                } else {
                    WorkspaceRootKind::GitRepository
                };
                return Ok((root, kind));
            }
        }
    }
    Ok((selected_path.to_path_buf(), WorkspaceRootKind::NonGit))
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn scan_consumer_scopes(root: &Path) -> Result<Vec<WorkspaceSkillScope>> {
    let mut scopes = Vec::new();
    for (consumer, directory) in [
        (DeploymentConsumer::Claude, ".claude"),
        (DeploymentConsumer::Codex, ".agents"),
    ] {
        let root_scope = root.join(directory).join("skills");
        if root_scope.is_dir() {
            scopes.push(WorkspaceSkillScope {
                consumer,
                path: root_scope.clone(),
                kind: WorkspaceScopeKind::RootLevel,
                skill_directories: child_names(&root_scope)?,
            });
        }
    }
    scan_nested_scopes(root, root, &mut scopes)?;
    scopes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(scopes)
}

fn scan_nested_scopes(
    root: &Path,
    current: &Path,
    scopes: &mut Vec<WorkspaceSkillScope>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if path == root.join(".git") {
            continue;
        }
        for (consumer, directory) in [
            (DeploymentConsumer::Claude, ".claude"),
            (DeploymentConsumer::Codex, ".agents"),
        ] {
            let candidate = path.join(directory).join("skills");
            if candidate.is_dir() && candidate != root.join(directory).join("skills") {
                scopes.push(WorkspaceSkillScope {
                    consumer,
                    path: candidate.clone(),
                    kind: WorkspaceScopeKind::NestedUnsupported,
                    skill_directories: child_names(&candidate)?,
                });
            }
        }
        scan_nested_scopes(root, &path, scopes)?;
    }
    Ok(())
}

fn child_names(path: &Path) -> Result<Vec<String>> {
    let mut names = fs::read_dir(path)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}
