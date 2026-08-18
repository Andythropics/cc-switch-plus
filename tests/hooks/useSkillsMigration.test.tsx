import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useApplySkillsMigration,
  useRestoreSkillsMigrationBackup,
  useResumeSkillsMigration,
} from "@/hooks/useSkills";

const { applyMock, resumeMock, restoreMock } = vi.hoisted(() => ({
  applyMock: vi.fn(),
  resumeMock: vi.fn(),
  restoreMock: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    applySkillsMigration: applyMock,
    resumeSkillsMigration: resumeMock,
    restoreSkillsMigrationBackup: restoreMock,
  },
}));

const expectedQueryKeys = [
  ["skills", "migrationPreflight"],
  ["skills", "library"],
  ["skills", "installed"],
  ["skills", "deployments"],
  ["skills", "deploymentRecovery"],
  ["skills", "unmanaged"],
  ["skills", "activity"],
  ["skills", "backups"],
];

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Skills migration mutation hooks", () => {
  beforeEach(() => {
    applyMock.mockReset();
    resumeMock.mockReset();
    restoreMock.mockReset();
  });

  it.each(["success", "error"] as const)(
    "refreshes every migration-dependent view after apply %s",
    async (settlement) => {
      const response = { outcome: "completed" };
      if (settlement === "success") applyMock.mockResolvedValueOnce(response);
      else applyMock.mockRejectedValueOnce(new Error("apply failed"));
      const queryClient = new QueryClient({
        defaultOptions: { mutations: { retry: false } },
      });
      const invalidateSpy = vi
        .spyOn(queryClient, "invalidateQueries")
        .mockResolvedValue(undefined);
      const { result } = renderHook(() => useApplySkillsMigration(), {
        wrapper: createWrapper(queryClient),
      });

      await act(async () => {
        const mutation = result.current.mutateAsync({
          observationToken: "migration-v1",
          preserveUnsupportedConsumerFiles: false,
        });
        if (settlement === "success")
          await expect(mutation).resolves.toBe(response);
        else await expect(mutation).rejects.toThrow("apply failed");
      });

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(8));
      for (const queryKey of expectedQueryKeys) {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
      }
    },
  );

  it.each(["success", "error"] as const)(
    "refreshes every migration-dependent view after resume %s",
    async (settlement) => {
      if (settlement === "success")
        resumeMock.mockResolvedValueOnce({ outcome: "resumable" });
      else resumeMock.mockRejectedValueOnce(new Error("resume failed"));
      const queryClient = new QueryClient({
        defaultOptions: { mutations: { retry: false } },
      });
      const invalidateSpy = vi
        .spyOn(queryClient, "invalidateQueries")
        .mockResolvedValue(undefined);
      const { result } = renderHook(() => useResumeSkillsMigration(), {
        wrapper: createWrapper(queryClient),
      });

      await act(async () => {
        const mutation = result.current.mutateAsync();
        if (settlement === "success")
          await expect(mutation).resolves.toBeDefined();
        else await expect(mutation).rejects.toThrow("resume failed");
      });

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(8));
      for (const queryKey of expectedQueryKeys) {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
      }
    },
  );

  it.each(["success", "error"] as const)(
    "refreshes every migration-dependent view after restore %s",
    async (settlement) => {
      if (settlement === "success")
        restoreMock.mockResolvedValueOnce({ outcome: "restored" });
      else restoreMock.mockRejectedValueOnce(new Error("restore failed"));
      const queryClient = new QueryClient({
        defaultOptions: { mutations: { retry: false } },
      });
      const invalidateSpy = vi
        .spyOn(queryClient, "invalidateQueries")
        .mockResolvedValue(undefined);
      const { result } = renderHook(() => useRestoreSkillsMigrationBackup(), {
        wrapper: createWrapper(queryClient),
      });

      await act(async () => {
        const mutation = result.current.mutateAsync("backup-v1");
        if (settlement === "success")
          await expect(mutation).resolves.toBeDefined();
        else await expect(mutation).rejects.toThrow("restore failed");
      });

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(8));
      for (const queryKey of expectedQueryKeys) {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
      }
    },
  );
});
