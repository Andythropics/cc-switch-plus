import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useApplySkillDeployments,
  useDeploymentRecovery,
  useRefreshSkillDeployments,
  useSkillDeployments,
} from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  inspectDeployments: vi.fn(),
  inspectDeploymentRecovery: vi.fn(),
  applyDeployments: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: apiMocks,
}));

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Skill Deployment hooks", () => {
  beforeEach(() => {
    apiMocks.inspectDeployments.mockReset();
    apiMocks.inspectDeploymentRecovery.mockReset();
    apiMocks.applyDeployments.mockReset();
  });

  it("loads desired and observed state through the deployment query", async () => {
    apiMocks.inspectDeployments.mockResolvedValueOnce({ items: [] });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () => useSkillDeployments({ consumer: "claude", workspace: "global" }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(apiMocks.inspectDeployments).toHaveBeenCalledWith({
      consumer: "claude",
      workspace: "global",
    });
    expect(result.current.data).toEqual({ items: [] });
  });

  it("invalidates deployment observations after apply so the UI refreshes", async () => {
    apiMocks.applyDeployments.mockResolvedValueOnce({
      items: [{ outcome: "applied" }],
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useApplySkillDeployments(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        intents: [
          {
            action: "deploy",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
          },
        ],
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deployments"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "activity"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deploymentRecovery"],
    });
  });

  it("loads recovery findings by stable scope and reconciles on focus", async () => {
    apiMocks.inspectDeploymentRecovery.mockResolvedValue({ findings: [] });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () =>
        useDeploymentRecovery({
          workspace: "project",
          workspaceId: "workspace-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(apiMocks.inspectDeploymentRecovery).toHaveBeenCalledWith({
      workspace: "project",
      workspaceId: "workspace-1",
    });

    window.dispatchEvent(new Event("focus"));
    await waitFor(() =>
      expect(apiMocks.inspectDeploymentRecovery).toHaveBeenCalledTimes(2),
    );
  });

  it("reconciles active observations when the window regains focus", async () => {
    apiMocks.inspectDeployments.mockResolvedValue({ items: [] });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    renderHook(
      () => useSkillDeployments({ consumer: "claude", workspace: "global" }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() =>
      expect(apiMocks.inspectDeployments).toHaveBeenCalledTimes(1),
    );
    window.dispatchEvent(new Event("focus"));
    await waitFor(() =>
      expect(apiMocks.inspectDeployments).toHaveBeenCalledTimes(2),
    );
  });

  it("supports an explicit manual reconciliation without replacing cached data", async () => {
    apiMocks.inspectDeployments.mockResolvedValue({ items: [] });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () => ({
        deployment: useSkillDeployments({
          consumer: "codex",
          workspace: "global",
        }),
        refresh: useRefreshSkillDeployments(),
      }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.deployment.isSuccess).toBe(true));
    await act(async () => {
      await result.current.refresh();
    });

    expect(apiMocks.inspectDeployments).toHaveBeenCalledTimes(2);
    expect(result.current.deployment.data).toEqual({ items: [] });
  });
});
