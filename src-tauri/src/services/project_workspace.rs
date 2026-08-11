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
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::database::Database;
use crate::services::skill_deployment::DeploymentConsumer;
use sha2::{Digest, Sha256};

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
    /// Stable registration identity used to validate a later relocation.
    ///
    /// This intentionally is not derived from the current filesystem path:
    /// moving a registered project must preserve its identity while a
    /// different repository/worktree must not be able to claim it.
    pub registration_fingerprint: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRelocationOutcome {
    Relocated,
    RegisteredDistinct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRelocation {
    pub outcome: WorkspaceRelocationOutcome,
    pub workspace: ProjectWorkspace,
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
        Self::ensure_supported_platform()?;
        let scan = Self::scan_path(selected_path)?;
        self.register_scan(scan, display_name)
    }

    fn register_scan(
        &self,
        scan: WorkspaceRegistrationScan,
        display_name: Option<String>,
    ) -> Result<WorkspaceRegistration> {
        let root = &scan.canonical_root;
        let registration_fingerprint = registration_fingerprint_for_root(root, scan.root_kind)?;
        for existing in self.db.list_project_workspaces()? {
            if paths_overlap(&existing.root_path, root) {
                return Err(anyhow!(
                    "Project Workspace overlaps registered root {}",
                    existing.root_path.display()
                ));
            }
            if !existing.registration_fingerprint.is_empty()
                && existing.registration_fingerprint == registration_fingerprint
            {
                return Err(anyhow!(
                    "Project Workspace identity is already registered at {}; use Relocate or Restore",
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
            registration_fingerprint,
            lifecycle: WorkspaceLifecycle::Active,
            created_at: now,
            updated_at: now,
        };
        self.db.save_project_workspace(&workspace)?;
        Ok(WorkspaceRegistration { workspace, scan })
    }

    /// List workspaces, optionally including archived identities. Active roots
    /// that have become unreachable are persisted as Unavailable before the
    /// response is returned, so the API and future mutations observe the same
    /// lifecycle state.
    pub fn list(&self, include_archived: bool) -> Result<Vec<ProjectWorkspace>> {
        Self::ensure_supported_platform()?;
        let workspaces = self.refresh_unavailable(self.db.list_project_workspaces()?)?;
        Ok(workspaces
            .into_iter()
            .filter(|workspace| {
                include_archived || workspace.lifecycle != WorkspaceLifecycle::Archived
            })
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<ProjectWorkspace>> {
        Self::ensure_supported_platform()?;
        Ok(self
            .db
            .get_project_workspace(id)?
            .map(|workspace| self.refresh_unavailable(vec![workspace]))
            .transpose()?
            .and_then(|mut workspaces| workspaces.pop()))
    }

    /// Rename a Workspace display label without changing its physical
    /// identity or any project content.
    pub fn rename(&self, id: &str, display_name: String) -> Result<ProjectWorkspace> {
        Self::ensure_supported_platform()?;
        let mut workspace = self
            .get(id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {id}"))?;
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(anyhow!("Project Workspace display name cannot be empty"));
        }
        workspace.display_name = display_name.to_string();
        workspace.updated_at = Utc::now().timestamp();
        self.db.update_project_workspace(&workspace)?;
        Ok(workspace)
    }

    /// Archive (unregister) an Active or Unavailable Workspace. This is
    /// metadata-only: links, project files, and local Git excludes are left
    /// exactly as they are.
    pub fn archive(&self, id: &str) -> Result<ProjectWorkspace> {
        Self::ensure_supported_platform()?;
        let mut workspace = self
            .get(id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {id}"))?;
        match workspace.lifecycle {
            WorkspaceLifecycle::Active | WorkspaceLifecycle::Unavailable => {
                workspace.lifecycle = WorkspaceLifecycle::Archived;
            }
            WorkspaceLifecycle::Archived => {
                return Err(anyhow!("Project Workspace is already archived"));
            }
        }
        workspace.updated_at = Utc::now().timestamp();
        self.db.update_project_workspace(&workspace)?;
        Ok(workspace)
    }

    /// Restore an Archived Workspace. A retained root that is still reachable
    /// returns Active; if the project remains away, the identity is restored
    /// as Unavailable and can later be relocated.
    pub fn restore(&self, id: &str) -> Result<ProjectWorkspace> {
        Self::ensure_supported_platform()?;
        let mut workspace = self
            .get(id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {id}"))?;
        if workspace.lifecycle != WorkspaceLifecycle::Archived {
            return Err(anyhow!(
                "Project Workspace can only be restored from Archived"
            ));
        }
        workspace.lifecycle =
            if workspace.root_path.is_dir() && workspace_root_matches_identity(&workspace)? {
                WorkspaceLifecycle::Active
            } else {
                WorkspaceLifecycle::Unavailable
            };
        workspace.updated_at = Utc::now().timestamp();
        self.db.update_project_workspace(&workspace)?;
        Ok(workspace)
    }

    /// Relocate an Unavailable Workspace after validating the replacement's
    /// canonical root and persisted filesystem/repository identity. A missing
    /// old root may retain the Workspace ID only when the full fingerprint
    /// matches; otherwise relocation is rejected so callers can explicitly
    /// register the candidate as a distinct Workspace. A reappeared old root
    /// always follows the structured distinct-registration outcome.
    pub fn relocate(&self, id: &str, new_path: &Path) -> Result<WorkspaceRelocation> {
        Self::ensure_supported_platform()?;
        let workspace = self
            .get(id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {id}"))?;
        if workspace.lifecycle != WorkspaceLifecycle::Unavailable {
            return Err(anyhow!(
                "Project Workspace relocation requires an Unavailable workspace"
            ));
        }
        let scan = Self::scan_path(new_path)?;
        let old_root_exists = path_node_exists(&workspace.root_path);
        let candidate_fingerprint =
            registration_fingerprint_for_root(&scan.canonical_root, scan.root_kind)?;

        // A still-present old path is a distinct registration rather than a
        // silent retargeting of this Workspace ID. When the old path is truly
        // gone, every root kind must prove the persisted identity; an
        // unrelated Non-Git directory can still be explicitly registered as a
        // separate Workspace through `register`.
        if old_root_exists {
            if scan.canonical_root == workspace.root_path {
                return Err(anyhow!(
                    "Project Workspace replacement path is already this registered root"
                ));
            }
            let registration = self.register_scan(scan, None)?;
            return Ok(WorkspaceRelocation {
                outcome: WorkspaceRelocationOutcome::RegisteredDistinct,
                workspace: registration.workspace,
            });
        }

        if scan.root_kind != workspace.root_kind {
            return Err(anyhow!(
                "Project Workspace replacement root kind does not match the registered identity"
            ));
        }
        if workspace.registration_fingerprint.is_empty()
            || candidate_fingerprint != workspace.registration_fingerprint
        {
            return Err(anyhow!(
                "Project Workspace replacement does not match the registered repository identity"
            ));
        }

        let mut relocated = workspace;
        relocated.root_path = scan.canonical_root;
        relocated.root_kind = scan.root_kind;
        relocated.registration_fingerprint = candidate_fingerprint;
        relocated.lifecycle = WorkspaceLifecycle::Active;
        relocated.updated_at = Utc::now().timestamp();
        self.db.update_project_workspace(&relocated)?;
        Ok(WorkspaceRelocation {
            outcome: WorkspaceRelocationOutcome::Relocated,
            workspace: relocated,
        })
    }

    /// Permanently forget an Archived Workspace identity only after all of
    /// its desired Project Deployments are gone. No filesystem operation is
    /// performed, so project content and any surviving unmanaged/managed links
    /// remain untouched for explicit Deployment actions.
    pub fn forget(&self, id: &str) -> Result<bool> {
        Self::ensure_supported_platform()?;
        let workspace = self
            .get(id)?
            .ok_or_else(|| anyhow!("Project Workspace not found: {id}"))?;
        if workspace.lifecycle != WorkspaceLifecycle::Archived {
            return Err(anyhow!(
                "Project Workspace can only be forgotten from Archived"
            ));
        }
        let deployments = self.db.list_skill_deployments()?;
        if deployments.iter().any(|deployment| {
            deployment.target.workspace == crate::services::skill_deployment::WorkspaceKind::Project
                && deployment.target.workspace_id == workspace.id
        }) {
            return Err(anyhow!(
                "Project Workspace still has desired Deployments; remove or explicitly forget them first"
            ));
        }
        self.db
            .delete_project_workspace(&workspace.id)
            .map_err(Into::into)
    }

    fn refresh_unavailable(
        &self,
        workspaces: Vec<ProjectWorkspace>,
    ) -> Result<Vec<ProjectWorkspace>> {
        let mut refreshed = Vec::with_capacity(workspaces.len());
        for mut workspace in workspaces {
            if workspace.lifecycle == WorkspaceLifecycle::Active
                && !workspace_root_matches_identity(&workspace)?
            {
                workspace.lifecycle = WorkspaceLifecycle::Unavailable;
                workspace.updated_at = Utc::now().timestamp();
                self.db.update_project_workspace(&workspace)?;
            }
            refreshed.push(workspace);
        }
        Ok(refreshed)
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

fn path_node_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Derive a stable identity marker for a canonical project root. The
/// device/inode pair protects against treating a clone or copied directory as
/// the same Workspace, while Git roots additionally use immutable repository
/// history and the linked-worktree admin path (relative to the shared Git
/// directory). A rename or `git worktree move` preserves the filesystem object
/// identity, while ordinary commits do not change the root-commit set.
pub(crate) fn registration_fingerprint_for_root(
    root: &Path,
    root_kind: WorkspaceRootKind,
) -> Result<String> {
    let metadata = fs::metadata(root)
        .with_context(|| format!("read Workspace root metadata {}", root.display()))?;
    let device = metadata.dev();
    let inode = metadata.ino();
    if root_kind == WorkspaceRootKind::NonGit {
        return Ok(format!("non_git:v2:{device}:{inode}"));
    }

    let mut root_commits = git_output(root, &["rev-list", "--max-parents=0", "--all"])?
        .lines()
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    root_commits.sort();
    let history = root_commits.join("\n");
    let common_dir = git_output(root, &["rev-parse", "--git-common-dir"])?;
    let git_dir = git_output(root, &["rev-parse", "--git-dir"])?;
    let common_dir = canonical_git_path(root, common_dir.trim())?;
    let git_dir = canonical_git_path(root, git_dir.trim())?;
    let admin_identity = if root_kind == WorkspaceRootKind::GitWorktree {
        git_dir
            .strip_prefix(&common_dir)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| git_dir.to_string_lossy().replace('\\', "/"))
    } else {
        "repository".to_string()
    };

    let mut hasher = Sha256::new();
    hasher.update(match root_kind {
        WorkspaceRootKind::GitRepository => b"git_repository\0" as &[u8],
        WorkspaceRootKind::GitWorktree => b"git_worktree\0" as &[u8],
        WorkspaceRootKind::NonGit => b"non_git\0" as &[u8],
    });
    hasher.update(device.to_le_bytes());
    hasher.update(inode.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(admin_identity.as_bytes());
    hasher.update(b"\0");
    hasher.update(history.as_bytes());
    if history.is_empty() {
        return Ok(format!("git:v2:{:x}:empty", hasher.finalize()));
    }
    Ok(format!("git:v2:{:x}", hasher.finalize()))
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", &root.display().to_string()])
        .args(args)
        .output()
        .with_context(|| format!("run git {:?}", args))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn canonical_git_path(root: &Path, path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::canonicalize(&path)
        .with_context(|| format!("canonicalize Git identity path {}", path.display()))
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
    if !workspace_root_matches_identity(&workspace)? {
        return Err(anyhow!("Project Workspace is unavailable"));
    }
    Ok(workspace
        .root_path
        .join(consumer_directory(consumer))
        .join("skills"))
}

/// Resolve a target root for safe Undeploy. Archived Workspaces may be
/// removed when their retained root still matches its persisted identity;
/// Unavailable or identity-mismatched roots are rejected without touching FS.
pub(crate) fn project_removal_target_root(
    db: &Database,
    workspace_id: &str,
    consumer: DeploymentConsumer,
) -> Result<PathBuf> {
    let workspace = db
        .get_project_workspace(workspace_id)?
        .ok_or_else(|| anyhow!("Project Workspace not found: {workspace_id}"))?;
    if workspace.lifecycle == WorkspaceLifecycle::Unavailable {
        return Err(anyhow!("Project Workspace is unavailable"));
    }
    if workspace.lifecycle != WorkspaceLifecycle::Active
        && workspace.lifecycle != WorkspaceLifecycle::Archived
    {
        return Err(anyhow!("Project Workspace cannot remove Deployments"));
    }
    if !workspace_root_matches_identity(&workspace)? {
        return Err(anyhow!("Project Workspace root identity is unavailable"));
    }
    Ok(workspace
        .root_path
        .join(consumer_directory(consumer))
        .join("skills"))
}

/// Resolve a project target for read-only reconciliation. Unlike mutation
/// target resolution this intentionally permits archived/unavailable rows so
/// inspection can report their stable lifecycle without changing the project.
pub(crate) fn project_observation_target_root(
    db: &Database,
    workspace_id: &str,
    consumer: DeploymentConsumer,
) -> Result<PathBuf> {
    let workspace = db
        .get_project_workspace(workspace_id)?
        .ok_or_else(|| anyhow!("Project Workspace not found: {workspace_id}"))?;
    Ok(workspace
        .root_path
        .join(consumer_directory(consumer))
        .join("skills"))
}

pub(crate) fn project_workspace_lifecycle(
    db: &Database,
    workspace_id: &str,
) -> Result<WorkspaceLifecycle> {
    let Some(mut workspace) = db.get_project_workspace(workspace_id)? else {
        return Err(anyhow!("Project Workspace not found: {workspace_id}"));
    };
    if workspace.lifecycle == WorkspaceLifecycle::Active
        && !workspace_root_matches_identity(&workspace)?
    {
        workspace.lifecycle = WorkspaceLifecycle::Unavailable;
    }
    Ok(workspace.lifecycle)
}

pub(crate) fn workspace_root_matches_identity(workspace: &ProjectWorkspace) -> Result<bool> {
    let root = fs::canonicalize(&workspace.root_path).ok();
    let Some(root) = root else {
        return Ok(false);
    };
    let Ok((resolved_root, resolved_kind)) = resolve_project_root(&root) else {
        return Ok(false);
    };
    if resolved_root != root || resolved_kind != workspace.root_kind {
        return Ok(false);
    }
    if workspace.registration_fingerprint.is_empty() {
        return Ok(false);
    }
    let Ok(candidate) = registration_fingerprint_for_root(&root, resolved_kind) else {
        return Ok(false);
    };
    Ok(candidate == workspace.registration_fingerprint)
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
