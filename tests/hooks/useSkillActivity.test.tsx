import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useSkillActivity } from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  listActivity: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({ skillsApi: apiMocks }));

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Skills activity hook", () => {
  beforeEach(() => apiMocks.listActivity.mockReset());

  it("loads newest-first pages and appends item entries on demand", async () => {
    apiMocks.listActivity
      .mockResolvedValueOnce({
        entries: [
          {
            id: 2,
            occurredAt: 2_000,
            operation: "deployment",
            reason: "deploy",
            detailCode: "none",
            outcome: "success",
            actor: "user",
            trigger: "batch",
            batch: { batchId: "batch-1", itemIndex: 0, itemCount: 2 },
          },
        ],
        nextCursor: { occurredAt: 1_000, id: 1 },
        hasMore: true,
      })
      .mockResolvedValueOnce({
        entries: [
          {
            id: 1,
            occurredAt: 1_000,
            operation: "deployment",
            reason: "deploy",
            detailCode: "partial_batch",
            outcome: "blocked",
            actor: "user",
            trigger: "batch",
            batch: { batchId: "batch-1", itemIndex: 1, itemCount: 2 },
          },
        ],
        hasMore: false,
      });

    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () => useSkillActivity({ operation: "deployment", limit: 50 }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.entries).toHaveLength(1));
    expect(apiMocks.listActivity).toHaveBeenCalledWith({
      operation: "deployment",
      limit: 50,
    });

    await act(async () => {
      await result.current.fetchNextPage();
    });

    expect(apiMocks.listActivity).toHaveBeenLastCalledWith({
      operation: "deployment",
      limit: 50,
      cursor: { occurredAt: 1_000, id: 1 },
    });
    await waitFor(() =>
      expect(result.current.entries.map((entry) => entry.id)).toEqual([2, 1]),
    );
    expect(result.current.hasNextPage).toBe(false);
  });

  it("refetches the active filter without retaining stale pages", async () => {
    apiMocks.listActivity.mockResolvedValue({
      entries: [],
      hasMore: false,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(
      () => useSkillActivity({ outcome: "failed", limit: 50 }),
      { wrapper: wrapper(queryClient) },
    );

    await waitFor(() => expect(apiMocks.listActivity).toHaveBeenCalledTimes(1));
    await act(async () => {
      await result.current.refetch();
    });

    expect(apiMocks.listActivity).toHaveBeenLastCalledWith({
      outcome: "failed",
      limit: 50,
    });
  });
});
