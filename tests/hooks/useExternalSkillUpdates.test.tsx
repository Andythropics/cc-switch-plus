import { act, renderHook, waitFor } from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useExternalSkillUpdates,
  useLinkLibrarySkillSource,
} from "@/hooks/useExternalSkillUpdates";
const { inspect, linkSource, apply } = vi.hoisted(() => ({
  inspect: vi.fn(),
  apply: vi.fn(),
  linkSource: vi.fn(),
}));
vi.mock("@/lib/api/externalSkillUpdates", () => ({
  externalSkillUpdatesApi: {
    inspect,
    link: vi.fn(),
    apply,
    linkSource,
  },
}));
beforeEach(() => {
  inspect.mockResolvedValue({
    candidates: [],
    observationToken: "a",
    warnings: [],
  });
  linkSource.mockResolvedValue({ id: "skill" });
});
afterEach(() => vi.useRealTimers());
function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    () => ({
      external: useExternalSkillUpdates(),
      manual: useLinkLibrarySkillSource(),
    }),
    { wrapper },
  );
  return { hook, client, wrapper };
}
describe("external update inspection lifecycle", () => {
  it("finishes the update while Library refresh continues in the background", async () => {
    apply.mockResolvedValue({ outcome: "updated" });
    const { hook, wrapper } = setup();
    let finishRefresh!: (value: string[]) => void;
    const refresh = vi.fn(
      () =>
        new Promise<string[]>((resolve) => {
          finishRefresh = resolve;
        }),
    );
    const library = renderHook(
      () =>
        useQuery({
          queryKey: ["skills", "library"],
          initialData: ["old"],
          staleTime: Infinity,
          queryFn: refresh,
        }),
      { wrapper },
    );
    act(() => {
      hook.result.current.external.apply.mutate({
        candidateId: "first",
        librarySkillId: "library",
        observationToken: "scoped",
        confirmLocalModifications: false,
        restoreDeployment: true,
      });
    });
    try {
      await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
      await waitFor(() =>
        expect(hook.result.current.external.apply.isSuccess).toBe(true),
      );
      expect(library.result.current.isFetching).toBe(true);
      expect(inspect).not.toHaveBeenCalled();
    } finally {
      await act(async () => {
        finishRefresh(["new"]);
      });
    }
    await waitFor(() => expect(library.result.current.data).toEqual(["new"]));
  });
  it.each(["updated", "up_to_date", "blocked", "stale", "recovery_required"])(
    "removes only completed updates from the retained snapshot: %s",
    async (outcome) => {
      const snapshot = {
        observationToken: "original",
        warnings: ["retained warning"],
        candidates: [{ id: "first" }, { id: "second" }],
      };
      inspect.mockResolvedValue(snapshot);
      apply.mockResolvedValue({ outcome });
      const { hook, client } = setup();
      await act(async () => {
        await hook.result.current.external.inspection.refetch();
      });
      await act(async () => {
        await hook.result.current.external.apply.mutateAsync({
          candidateId: "first",
          librarySkillId: "library",
          observationToken: "scoped",
          confirmLocalModifications: false,
          restoreDeployment: true,
        });
      });
      expect(client.getQueryData(["skills", "externalUpdates"])).toEqual({
        ...snapshot,
        candidates: ["updated", "up_to_date"].includes(outcome)
          ? [{ id: "second" }]
          : snapshot.candidates,
      });
      expect(inspect).toHaveBeenCalledTimes(1);
    },
  );
  it("never scans on entry, focus, timer, invalidation, or remount", async () => {
    vi.useFakeTimers();
    const { hook, client, wrapper } = setup();
    expect(inspect).not.toHaveBeenCalled();
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(65_000);
      await client.invalidateQueries({ queryKey: ["skills"] });
    });
    expect(inspect).not.toHaveBeenCalled();
    hook.unmount();
    const remounted = renderHook(() => useExternalSkillUpdates(), { wrapper });
    expect(inspect).not.toHaveBeenCalled();
    remounted.unmount();
  });
  it("scans only on an explicit request and does not rescan after linking a card", async () => {
    const { hook } = setup();
    await act(async () => {
      await hook.result.current.external.inspection.refetch();
    });
    expect(inspect).toHaveBeenCalledTimes(1);
    await act(async () => {
      await hook.result.current.manual.mutateAsync({
        librarySkillId: "skill",
        source: { kind: "git", repoOwner: "a", repoName: "b", skillPath: "." },
        expectedContentHash: "baseline",
      });
      window.dispatchEvent(new Event("focus"));
    });
    expect(inspect).toHaveBeenCalledTimes(1);
  });
});
