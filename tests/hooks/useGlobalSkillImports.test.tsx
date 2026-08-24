import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useApplyGlobalSkillImport,
  useInspectGlobalSkillImports,
} from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({ inspect: vi.fn(), apply: vi.fn() }));

vi.mock("@/lib/api/globalSkillImports", () => ({
  globalSkillImportsApi: apiMocks,
}));
vi.mock("@/lib/api/skills", () => ({ skillsApi: {} }));
vi.mock("@/lib/api/projectWorkspaces", () => ({ projectWorkspacesApi: {} }));

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

describe("Global Skill Import hooks", () => {
  beforeEach(() => {
    apiMocks.inspect.mockReset().mockResolvedValue({
      observationToken: "global-token",
      findings: [],
    });
    apiMocks.apply.mockReset().mockResolvedValue({
      findingId: "global:finding",
      outcome: "created",
    });
  });

  it("loads recurring Global findings with the dedicated query key", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(() => useInspectGlobalSkillImports(), {
      wrapper: wrapper(client),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(apiMocks.inspect).toHaveBeenCalledTimes(1);
    expect(client.getQueryData(["skills", "globalSkillImports"])).toEqual({
      observationToken: "global-token",
      findings: [],
    });
  });

  it("invalidates Global findings, Library, deployments, and Activity", async () => {
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useApplyGlobalSkillImport(), {
      wrapper: wrapper(client),
    });
    const intent = {
      findingId: "global:finding",
      observationToken: "global-token",
      mode: "import_only" as const,
      resolution: {
        kind: "create_new" as const,
        directory: "global-skill",
      },
    };

    await act(async () => {
      await result.current.mutateAsync(intent);
    });

    expect(apiMocks.apply).toHaveBeenCalledWith(intent);
    for (const queryKey of [
      ["skills", "globalSkillImports"],
      ["skills", "library"],
      ["skills", "deployments"],
      ["skills", "activity"],
    ]) {
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
    }
  });
});
