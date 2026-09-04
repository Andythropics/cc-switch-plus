import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createRef, StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ProjectWorkspacesPanel,
  type ProjectWorkspacesPanelHandle,
} from "@/components/skills/ProjectWorkspacesPanel";

const apiMocks = vi.hoisted(() => ({
  listWorkspaces: vi.fn(),
  inspectSkillImports: vi.fn(),
  getLibrary: vi.fn(),
  inspectDeployments: vi.fn(),
  inspectDeploymentRecovery: vi.fn(),
}));

vi.mock("@/lib/api/projectWorkspaces", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/projectWorkspaces")>();
  return {
    ...actual,
    projectWorkspacesApi: {
      ...actual.projectWorkspacesApi,
      list: apiMocks.listWorkspaces,
      inspectSkillImports: apiMocks.inspectSkillImports,
    },
  };
});

vi.mock("@/lib/api/skills", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/skills")>();
  return {
    ...actual,
    skillsApi: {
      ...actual.skillsApi,
      getLibrary: apiMocks.getLibrary,
      inspectDeployments: apiMocks.inspectDeployments,
      inspectDeploymentRecovery: apiMocks.inspectDeploymentRecovery,
    },
  };
});

const workspaces = [
  {
    id: "workspace-a",
    displayName: "Workspace A",
    rootPath: "/tmp/workspace-a",
    rootKind: "git_repository" as const,
    lifecycle: "active" as const,
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: "workspace-b",
    displayName: "Workspace B",
    rootPath: "/tmp/workspace-b",
    rootKind: "git_repository" as const,
    lifecycle: "active" as const,
    createdAt: 2,
    updatedAt: 2,
  },
];

function callsForWorkspace(
  mock: ReturnType<typeof vi.fn>,
  workspaceId: string,
) {
  return mock.mock.calls.filter(
    ([query]) => query?.workspaceId === workspaceId,
  );
}

function renderPanel(queryClient: QueryClient) {
  const panelRef = createRef<ProjectWorkspacesPanelHandle>();
  const view = render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <ProjectWorkspacesPanel ref={panelRef} />
      </QueryClientProvider>
    </StrictMode>,
  );
  return { ...view, panelRef };
}

async function settle(queryClient: QueryClient) {
  await waitFor(() => expect(queryClient.isFetching()).toBe(0));
}

describe("Project Workspaces inspection session", () => {
  beforeEach(() => {
    apiMocks.listWorkspaces.mockReset().mockResolvedValue(workspaces);
    apiMocks.inspectSkillImports.mockReset().mockImplementation((workspaceId) =>
      Promise.resolve({
        workspaceId,
        observationToken: `imports-${workspaceId}`,
        findings: [],
      }),
    );
    apiMocks.getLibrary.mockReset().mockResolvedValue([]);
    apiMocks.inspectDeployments.mockReset().mockResolvedValue({ items: [] });
    apiMocks.inspectDeploymentRecovery
      .mockReset()
      .mockResolvedValue({ findings: [] });
  });

  it("inspects A only once across A to B to A in one mounted page session", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    renderPanel(queryClient);
    await settle(queryClient);

    fireEvent.click(screen.getByRole("button", { name: /Workspace B/ }));
    await settle(queryClient);
    fireEvent.click(screen.getByRole("button", { name: /Workspace A/ }));
    await settle(queryClient);

    expect(apiMocks.inspectSkillImports).toHaveBeenCalledTimes(2);
    expect(apiMocks.inspectSkillImports).toHaveBeenCalledWith("workspace-a");
    expect(apiMocks.inspectSkillImports).toHaveBeenCalledWith("workspace-b");
    expect(
      callsForWorkspace(apiMocks.inspectDeployments, "workspace-a"),
    ).toHaveLength(2);
    expect(
      callsForWorkspace(apiMocks.inspectDeployments, "workspace-b"),
    ).toHaveLength(2);
  });

  it("starts a fresh cycle after remounting with the same QueryClient", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const first = renderPanel(queryClient);
    await settle(queryClient);
    first.unmount();
    await waitFor(() => {
      expect(
        queryClient
          .getQueryCache()
          .findAll({ queryKey: ["skills", "projectSkillImports"] }),
      ).toHaveLength(0);
      expect(
        queryClient
          .getQueryCache()
          .findAll({ queryKey: ["skills", "deployments"] }),
      ).toHaveLength(0);
    });

    renderPanel(queryClient);
    await settle(queryClient);

    expect(
      apiMocks.inspectSkillImports.mock.calls.filter(
        ([workspaceId]) => workspaceId === "workspace-a",
      ),
    ).toHaveLength(2);
    expect(
      callsForWorkspace(apiMocks.inspectDeployments, "workspace-a"),
    ).toHaveLength(4);
  });

  it("Refresh refreshes A now and makes cached B refresh on next selection", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { panelRef } = renderPanel(queryClient);
    await settle(queryClient);
    fireEvent.click(screen.getByRole("button", { name: /Workspace B/ }));
    await settle(queryClient);
    fireEvent.click(screen.getByRole("button", { name: /Workspace A/ }));
    await settle(queryClient);

    await act(async () => panelRef.current?.refresh());
    await settle(queryClient);

    expect(
      apiMocks.inspectSkillImports.mock.calls.filter(
        ([workspaceId]) => workspaceId === "workspace-a",
      ),
    ).toHaveLength(2);
    expect(
      callsForWorkspace(apiMocks.inspectDeployments, "workspace-a"),
    ).toHaveLength(4);
    expect(
      apiMocks.inspectSkillImports.mock.calls.filter(
        ([workspaceId]) => workspaceId === "workspace-b",
      ),
    ).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: /Workspace B/ }));
    await settle(queryClient);

    expect(
      apiMocks.inspectSkillImports.mock.calls.filter(
        ([workspaceId]) => workspaceId === "workspace-b",
      ),
    ).toHaveLength(2);
    expect(
      callsForWorkspace(apiMocks.inspectDeployments, "workspace-b"),
    ).toHaveLength(4);
  });
});
