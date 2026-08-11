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

  async list(): Promise<ProjectWorkspace[]> {
    return await invoke("list_project_workspaces");
  },
};
