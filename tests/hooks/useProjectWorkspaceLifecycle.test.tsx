import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useArchiveProjectWorkspace,
  useApplyProjectSkillImport,
  useInspectProjectSkillImports,
  useProjectWorkspaces,
  useRenameProjectWorkspace,
} from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  list: vi.fn(),
  rename: vi.fn(),
  archive: vi.fn(),
  restore: vi.fn(),
  relocate: vi.fn(),
  forget: vi.fn(),
  inspectSkillImports: vi.fn(),
  applySkillImport: vi.fn(),
}));

vi.mock("@/lib/api/projectWorkspaces", () => ({
  projectWorkspacesApi: apiMocks,
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {},
}));

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Project Workspace lifecycle hooks", () => {
  beforeEach(() => {
    apiMocks.list.mockReset().mockResolvedValue([]);
    apiMocks.rename.mockReset().mockResolvedValue({});
    apiMocks.archive.mockReset().mockResolvedValue({});
    apiMocks.restore.mockReset().mockResolvedValue({});
    apiMocks.relocate.mockReset().mockResolvedValue({});
    apiMocks.forget.mockReset().mockResolvedValue(true);
    apiMocks.inspectSkillImports.mockReset().mockResolvedValue({
      workspaceId: "workspace-1",
      observationToken: "scan-token",
      findings: [],
    });
    apiMocks.applySkillImport.mockReset().mockResolvedValue({
      findingId: "finding-1",
      outcome: "created",
    });
  });

  it("loads archived identities explicitly for the lifecycle UI", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(() => useProjectWorkspaces(), {
      wrapper: wrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(apiMocks.list).toHaveBeenCalledWith(true);
  });

  it("invalidates workspace and deployment observations after lifecycle mutation", async () => {
    apiMocks.rename.mockResolvedValueOnce({ id: "workspace-1" });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useRenameProjectWorkspace(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        workspaceId: "workspace-1",
        displayName: "Renamed",
      });
    });

    expect(apiMocks.rename).toHaveBeenCalledWith("workspace-1", "Renamed");
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "projectWorkspaces"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deployments"],
    });
  });

  it("uses the archive mutation without implying project deletion", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const { result } = renderHook(() => useArchiveProjectWorkspace(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync("workspace-1");
    });

    expect(apiMocks.archive).toHaveBeenCalledWith("workspace-1");
  });

  it("loads workspace import findings through an observation-token query", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () => useInspectProjectSkillImports("workspace-1"),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(apiMocks.inspectSkillImports).toHaveBeenCalledWith("workspace-1");
  });

  it("invalidates import findings, Library, and deployments after import apply", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useApplyProjectSkillImport(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        workspaceId: "workspace-1",
        findingId: "finding-1",
        observationToken: "scan-token",
        mode: "import_only",
        resolution: { kind: "reuse", librarySkillId: "library-1" },
      });
    });

    expect(apiMocks.applySkillImport).toHaveBeenCalledWith({
      workspaceId: "workspace-1",
      findingId: "finding-1",
      observationToken: "scan-token",
      mode: "import_only",
      resolution: { kind: "reuse", librarySkillId: "library-1" },
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "projectSkillImports", "workspace-1"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "library"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deployments"],
    });
  });
});
