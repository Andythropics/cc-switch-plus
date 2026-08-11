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

  it("lists registered workspaces without accepting a caller-supplied root", async () => {
    await projectWorkspacesApi.list();

    expect(invokeMock).toHaveBeenCalledWith("list_project_workspaces");
    expect(invokeMock.mock.calls[0][1]).toBeUndefined();
  });
});
