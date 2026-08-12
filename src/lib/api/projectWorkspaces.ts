import { invoke } from "@tauri-apps/api/core";

import type { DeploymentConsumer, LibrarySkillCompatibility } from "./skills";

export type WorkspaceRootKind = "git_repository" | "git_worktree" | "non_git";
export type WorkspaceLifecycle = "active" | "archived" | "unavailable";
export type WorkspaceScopeKind = "root_level" | "nested_unsupported";

export interface ProjectWorkspace {
  id: string;
  displayName: string;
  rootPath: string;
  rootKind: WorkspaceRootKind;
  lifecycle: WorkspaceLifecycle;
  createdAt: number;
  updatedAt: number;
}

export interface WorkspaceSkillScope {
  consumer: DeploymentConsumer;
  path: string;
  kind: WorkspaceScopeKind;
  skillDirectories: string[];
}

export interface WorkspaceRegistrationScan {
  selectedPath: string;
  canonicalRoot: string;
  rootKind: WorkspaceRootKind;
  scopes: WorkspaceSkillScope[];
}

export interface WorkspaceRegistration {
  workspace: ProjectWorkspace;
  scan: WorkspaceRegistrationScan;
}

export type ProjectSkillImportFindingScope =
  | "root_level"
  | "nested_unsupported";

export type ProjectSkillImportValidationStatus = "valid" | "invalid";

export interface ProjectSkillImportValidation {
  status: ProjectSkillImportValidationStatus;
  issues: string[];
  displayName?: string;
  description?: string;
}

export interface ProjectSkillImportLibraryMatch {
  kind: "none" | "identical" | "different";
  librarySkillId?: string;
  displayName?: string;
  directory?: string;
}

export interface ProjectSkillImportGitState {
  tracked: boolean;
  paths: string[];
}

export type ProjectSkillImportReplaceBlockReason =
  | "directory_identity_mismatch"
  | "git_tracked_content"
  | "nested_unsupported"
  | "invalid_source";

export interface ProjectSkillImportReplaceEligibility {
  eligible: boolean;
  reason?: ProjectSkillImportReplaceBlockReason;
}

export interface ProjectSkillImportDirectoryCollision {
  kind: "none" | "library" | "invalid" | "reserved";
  requested: string;
  suggestions: string[];
}

export interface ProjectSkillImportFinding {
  id: string;
  consumer: DeploymentConsumer;
  scope: ProjectSkillImportFindingScope;
  /** Display-only source location; apply binds by workspace, finding, token. */
  sourcePath: string;
  directory: string;
  validation: ProjectSkillImportValidation;
  compatibility: LibrarySkillCompatibility;
  libraryMatch: ProjectSkillImportLibraryMatch;
  git: ProjectSkillImportGitState;
  directoryCollision: ProjectSkillImportDirectoryCollision;
  replaceEligibility: ProjectSkillImportReplaceEligibility;
}

export interface ProjectSkillImportInspection {
  workspaceId: string;
  observationToken: string;
  findings: ProjectSkillImportFinding[];
}

export type ProjectSkillImportMode = "import_only" | "import_and_replace";

export type ProjectSkillImportResolution =
  | { kind: "reuse"; librarySkillId: string }
  | { kind: "create_new"; directory: string; displayName?: string }
  | {
      kind: "replace_library";
      librarySkillId: string;
      confirmed: true;
    };

export interface ProjectSkillImportIntent {
  workspaceId: string;
  findingId: string;
  observationToken: string;
  mode: ProjectSkillImportMode;
  resolution: ProjectSkillImportResolution;
}

export type ProjectSkillImportOutcome =
  | "reused"
  | "created"
  | "library_replaced"
  | "deployed"
  | "blocked"
  | "stale"
  | "rolled_back"
  | "recovery_required";

export interface ProjectSkillImportResult {
  findingId: string;
  outcome: ProjectSkillImportOutcome;
  librarySkillId?: string;
  directory?: string;
  reason?: ProjectSkillImportReplaceBlockReason;
  message?: string;
  backupPath?: string;
}

export type WorkspaceRelocationOutcome = "relocated" | "registered_distinct";

export interface WorkspaceRelocation {
  outcome: WorkspaceRelocationOutcome;
  workspace: ProjectWorkspace;
}

export const projectWorkspacesApi = {
  async inspect(path: string): Promise<WorkspaceRegistrationScan> {
    return await invoke("inspectProjectWorkspace", { path });
  },

  async register(
    path: string,
    displayName?: string,
  ): Promise<WorkspaceRegistration> {
    return await invoke("registerProjectWorkspace", {
      path,
      displayName: displayName?.trim() || null,
    });
  },

  async list(includeArchived = false): Promise<ProjectWorkspace[]> {
    return await invoke("listProjectWorkspaces", { includeArchived });
  },

  async rename(
    workspaceId: string,
    displayName: string,
  ): Promise<ProjectWorkspace> {
    return await invoke("renameProjectWorkspace", {
      workspaceId,
      displayName: displayName.trim(),
    });
  },

  async archive(workspaceId: string): Promise<ProjectWorkspace> {
    return await invoke("archiveProjectWorkspace", { workspaceId });
  },

  async restore(workspaceId: string): Promise<ProjectWorkspace> {
    return await invoke("restoreProjectWorkspace", { workspaceId });
  },

  async relocate(
    workspaceId: string,
    path: string,
  ): Promise<WorkspaceRelocation> {
    return await invoke("relocateProjectWorkspace", { workspaceId, path });
  },

  async forget(workspaceId: string): Promise<boolean> {
    return await invoke("forgetProjectWorkspace", { workspaceId });
  },

  async inspectSkillImports(
    workspaceId: string,
  ): Promise<ProjectSkillImportInspection> {
    return await invoke("inspectProjectSkillImports", { workspaceId });
  },

  async applySkillImport(
    intent: ProjectSkillImportIntent,
  ): Promise<ProjectSkillImportResult> {
    return await invoke("applyProjectSkillImport", { intent });
  },
};
