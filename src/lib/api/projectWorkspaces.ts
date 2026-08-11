import { invoke } from "@tauri-apps/api/core";

import type { DeploymentConsumer } from "./skills";

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

export type WorkspaceRelocationOutcome = "relocated" | "registered_distinct";

export interface WorkspaceRelocation {
  outcome: WorkspaceRelocationOutcome;
  workspace: ProjectWorkspace;
}

export const projectWorkspacesApi = {
  async inspect(path: string): Promise<WorkspaceRegistrationScan> {
    return await invoke("inspect_project_workspace", { path });
  },

  async register(
    path: string,
    displayName?: string,
  ): Promise<WorkspaceRegistration> {
    return await invoke("register_project_workspace", {
      path,
      displayName: displayName?.trim() || null,
    });
  },

  async list(includeArchived = false): Promise<ProjectWorkspace[]> {
    return await invoke("list_project_workspaces", { includeArchived });
  },

  async rename(
    workspaceId: string,
    displayName: string,
  ): Promise<ProjectWorkspace> {
    return await invoke("rename_project_workspace", {
      workspaceId,
      displayName: displayName.trim(),
    });
  },

  async archive(workspaceId: string): Promise<ProjectWorkspace> {
    return await invoke("archive_project_workspace", { workspaceId });
  },

  async restore(workspaceId: string): Promise<ProjectWorkspace> {
    return await invoke("restore_project_workspace", { workspaceId });
  },

  async relocate(
    workspaceId: string,
    path: string,
  ): Promise<WorkspaceRelocation> {
    return await invoke("relocate_project_workspace", { workspaceId, path });
  },

  async forget(workspaceId: string): Promise<boolean> {
    return await invoke("forget_project_workspace", { workspaceId });
  },
};
