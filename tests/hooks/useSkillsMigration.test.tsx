import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useAcknowledgeSkillsMigrationReport,
  useApplySkillsMigration,
  useRevealSkillsMigrationFinding,
  useRestoreSkillsMigrationBackup,
  useResumeSkillsMigration,
  useSkillsMigrationReport,
} from "@/hooks/useSkills";

const {
  acknowledgeMock,
  applyMock,
  inspectReportMock,
  revealFindingMock,
  resumeMock,
  restoreMock,
} = vi.hoisted(() => ({
  acknowledgeMock: vi.fn(),
  applyMock: vi.fn(),
  inspectReportMock: vi.fn(),
  revealFindingMock: vi.fn(),
  resumeMock: vi.fn(),
  restoreMock: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    acknowledgeSkillsMigrationReport: acknowledgeMock,
    applySkillsMigration: applyMock,
    inspectLatestSkillsMigrationReport: inspectReportMock,
    revealSkillsMigrationFinding: revealFindingMock,
    resumeSkillsMigration: resumeMock,
    restoreSkillsMigrationBackup: restoreMock,
  },
}));

const expectedQueryKeys = [
  ["skills", "migrationPreflight"],
  ["skills", "migrationReport"],
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
    acknowledgeMock.mockReset();
    applyMock.mockReset();
    inspectReportMock.mockReset();
    revealFindingMock.mockReset();
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

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(9));
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

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(9));
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

      await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(9));
      for (const queryKey of expectedQueryKeys) {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
      }
    },
  );

  it("loads the latest durable migration report only when enabled", async () => {
    const report = {
      runId: "run-opaque",
      state: "completed",
      createdAt: 1,
      summary: { performed: 2, preserved: 1, open: 0 },
      findings: [],
    };
    inspectReportMock.mockResolvedValueOnce(report);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result, rerender } = renderHook(
      ({ enabled }) => useSkillsMigrationReport({ enabled }),
      {
        initialProps: { enabled: false },
        wrapper: createWrapper(queryClient),
      },
    );

    expect(inspectReportMock).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await waitFor(() => expect(result.current.data).toEqual(report));
    expect(inspectReportMock).toHaveBeenCalledTimes(1);
  });

  it("acknowledges a report and invalidates the report alongside migration views", async () => {
    const report = {
      runId: "run-opaque",
      state: "completed",
      createdAt: 1,
      summary: { performed: 1, preserved: 1, open: 0 },
      findings: [],
    };
    acknowledgeMock.mockResolvedValueOnce(report);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);
    const { result } = renderHook(() => useAcknowledgeSkillsMigrationReport(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await expect(result.current.mutateAsync("run-opaque")).resolves.toBe(
        report,
      );
    });

    expect(acknowledgeMock).toHaveBeenCalledWith("run-opaque");
    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(9));
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "migrationReport"],
    });
  });

  it("reveals a finding by opaque identity without accepting a filesystem path", async () => {
    revealFindingMock.mockResolvedValueOnce(true);
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useRevealSkillsMigrationFinding(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await expect(result.current.mutateAsync("finding-run-opaque-1")).resolves.toBe(
        true,
      );
    });

    expect(revealFindingMock).toHaveBeenCalledTimes(1);
    expect(revealFindingMock).toHaveBeenCalledWith("finding-run-opaque-1");
    expect(revealFindingMock).not.toHaveBeenCalledWith(
      expect.stringContaining("/"),
    );
  });
});
