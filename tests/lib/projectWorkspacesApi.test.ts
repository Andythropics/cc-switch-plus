import { beforeEach, describe, expect, it, vi } from "vitest";

import { projectWorkspacesApi } from "@/lib/api/projectWorkspaces";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("Project Workspace API", () => {
  beforeEach(() => invokeMock.mockReset());

  it("inspects a selected path through the registration seam", async () => {
    await projectWorkspacesApi.inspect("/tmp/repo");

    expect(invokeMock).toHaveBeenCalledWith("inspect_project_workspace", {
      path: "/tmp/repo",
    });
  });

  it("registers a workspace with an optional display name", async () => {
    await projectWorkspacesApi.register("/tmp/repo", "  Demo repo  ");

    expect(invokeMock).toHaveBeenCalledWith("register_project_workspace", {
      path: "/tmp/repo",
      displayName: "Demo repo",
    });
  });

  it("passes null when a display name is omitted", async () => {
    await projectWorkspacesApi.register("/tmp/repo", "   ");

    expect(invokeMock).toHaveBeenCalledWith("register_project_workspace", {
      path: "/tmp/repo",
      displayName: null,
    });
  });

  it("lists registered workspaces with an explicit archived-row policy", async () => {
    await projectWorkspacesApi.list(true);

    expect(invokeMock).toHaveBeenCalledWith("list_project_workspaces", {
      includeArchived: true,
    });
  });

  it("exposes lifecycle mutations through explicit workspace commands", async () => {
    await projectWorkspacesApi.rename("workspace-1", "Renamed");
    expect(invokeMock).toHaveBeenLastCalledWith("rename_project_workspace", {
      workspaceId: "workspace-1",
      displayName: "Renamed",
    });

    await projectWorkspacesApi.archive("workspace-1");
    expect(invokeMock).toHaveBeenLastCalledWith("archive_project_workspace", {
      workspaceId: "workspace-1",
    });

    await projectWorkspacesApi.restore("workspace-1");
    expect(invokeMock).toHaveBeenLastCalledWith("restore_project_workspace", {
      workspaceId: "workspace-1",
    });

    await projectWorkspacesApi.relocate("workspace-1", "/tmp/new-root");
    expect(invokeMock).toHaveBeenLastCalledWith("relocate_project_workspace", {
      workspaceId: "workspace-1",
      path: "/tmp/new-root",
    });

    await projectWorkspacesApi.forget("workspace-1");
    expect(invokeMock).toHaveBeenLastCalledWith("forget_project_workspace", {
      workspaceId: "workspace-1",
    });
  });

  it("preserves the structured relocation outcome from the backend", async () => {
    invokeMock.mockResolvedValueOnce({
      outcome: "registered_distinct",
      workspace: { id: "workspace-distinct" },
    });

    const result = await projectWorkspacesApi.relocate(
      "workspace-1",
      "/tmp/candidate",
    );

    expect(result.outcome).toBe("registered_distinct");
    expect(result.workspace.id).toBe("workspace-distinct");
  });
});
