import type { PropsWithChildren } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useApplyLibrarySkillUpdate,
  useCheckLibrarySkillUpdate,
  useDeleteLibrarySkill,
  useInspectLibrarySkillDeletion,
} from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  checkLibrarySkillUpdate: vi.fn(),
  applyLibrarySkillUpdate: vi.fn(),
  inspectLibrarySkillDeletion: vi.fn(),
  deleteLibrarySkill: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({ skillsApi: apiMocks }));

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Library update and deletion hooks", () => {
  beforeEach(() => {
    Object.values(apiMocks).forEach((mock) => mock.mockReset());
  });

  it("caches a check/stage result by Library Skill identity", async () => {
    const result = {
      librarySkillId: "library-1",
      outcome: "update_available",
      observationToken: "observation-1",
      stageToken: "stage-1",
      recordedContentHash: "recorded",
      stagedContentHash: "staged",
      localModified: false,
      affectedDeployments: [],
    };
    apiMocks.checkLibrarySkillUpdate.mockResolvedValueOnce(result);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const { result: hook } = renderHook(() => useCheckLibrarySkillUpdate(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await hook.current.mutateAsync("library-1");
    });

    expect(
      queryClient.getQueryData(["skills", "libraryUpdate", "library-1"]),
    ).toEqual(result);
  });

  it("invalidates Library, deployments, and the staged check after update apply", async () => {
    apiMocks.applyLibrarySkillUpdate.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "updated",
      affectedDeployments: [],
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useApplyLibrarySkillUpdate(), {
      wrapper: wrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        librarySkillId: "library-1",
        observationToken: "observation-1",
        stageToken: "stage-1",
        confirmLocalModifications: false,
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "library"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deployments"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "libraryUpdate", "library-1"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "activity"],
    });
  });

  it("invalidates Library and deployment observations after delete, while inspection stays explicit", async () => {
    apiMocks.inspectLibrarySkillDeletion.mockResolvedValueOnce({
      librarySkillId: "library-1",
      observationToken: "delete-observation",
      targets: [],
      blocked: false,
    });
    apiMocks.deleteLibrarySkill.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "deleted",
      items: [],
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(
      () => ({
        inspect: useInspectLibrarySkillDeletion(),
        remove: useDeleteLibrarySkill(),
      }),
      { wrapper: wrapper(queryClient) },
    );

    await act(async () => {
      await result.current.inspect.mutateAsync("library-1");
      await result.current.remove.mutateAsync({
        librarySkillId: "library-1",
        observationToken: "delete-observation",
      });
    });

    expect(apiMocks.inspectLibrarySkillDeletion).toHaveBeenCalledWith(
      "library-1",
    );
    expect(apiMocks.deleteLibrarySkill).toHaveBeenCalledWith({
      librarySkillId: "library-1",
      observationToken: "delete-observation",
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "library"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "deployments"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "activity"],
    });
  });
});
