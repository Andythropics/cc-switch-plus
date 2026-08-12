import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsActivityPanel } from "@/components/skills/SkillsActivityPanel";
import type { LibrarySkill, SkillActivityEntry } from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

const { activityQueryMock, refetchMock, fetchNextPageMock, activityState } =
  vi.hoisted(() => ({
    activityQueryMock: vi.fn(),
    refetchMock: vi.fn(),
    fetchNextPageMock: vi.fn(),
    activityState: {
      entries: [] as SkillActivityEntry[],
      isPending: false,
      isFetching: false,
      isFetchingNextPage: false,
      isError: false,
      error: null as Error | null,
      hasNextPage: false,
    },
  }));

const librarySkill: LibrarySkill = {
  id: "library-1",
  directory: "careful-review",
  displayName: "Careful review",
  description: "Review changes",
  source: { kind: "local_import" },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: "hash",
  acquiredAt: 1,
  updatedAt: 1,
};

const workspace: ProjectWorkspace = {
  id: "workspace-1",
  displayName: "Demo workspace",
  rootPath: "/private/project",
  rootKind: "git_repository",
  lifecycle: "active",
  createdAt: 1,
  updatedAt: 1,
};

vi.mock("@/hooks/useSkills", () => ({
  useSkillActivity: (query: unknown) => {
    activityQueryMock(query);
    return {
      ...activityState,
      refetch: refetchMock,
      fetchNextPage: fetchNextPageMock,
    };
  },
  useLibrarySkills: () => ({ data: [librarySkill] }),
  useProjectWorkspaces: () => ({ data: [workspace] }),
}));

const makeEntry = (
  overrides: Partial<SkillActivityEntry> = {},
): SkillActivityEntry => ({
  id: 1,
  occurredAt: 2_000,
  operation: "deployment",
  reason: "deploy",
  detailCode: "none",
  outcome: "success",
  actor: "user",
  trigger: "batch",
  ...overrides,
});

describe("SkillsActivityPanel", () => {
  beforeEach(() => {
    activityQueryMock.mockReset();
    refetchMock.mockReset().mockResolvedValue(undefined);
    fetchNextPageMock.mockReset().mockResolvedValue(undefined);
    activityState.entries = [];
    activityState.isPending = false;
    activityState.isFetching = false;
    activityState.isFetchingNextPage = false;
    activityState.isError = false;
    activityState.error = null;
    activityState.hasNextPage = false;
  });

  it("groups batch items, preserves item order, and links stable identities", async () => {
    activityState.entries = [
      makeEntry({
        id: 2,
        occurredAt: 2_000,
        outcome: "blocked",
        reason: "recover_deployment",
        detailCode: "partial_batch",
        target: {
          librarySkillId: "library-1",
          workspaceId: "workspace-1",
          workspaceKind: "project",
          consumer: "claude",
        },
        batch: { batchId: "batch-1", itemIndex: 1, itemCount: 2 },
      }),
      makeEntry({
        id: 3,
        occurredAt: 3_000,
        outcome: "success",
        target: { workspaceKind: "global", consumer: "codex" },
        batch: { batchId: "batch-1", itemIndex: 0, itemCount: 2 },
      }),
      makeEntry({
        id: 4,
        operation: "removal",
        reason: "library_remove",
        outcome: "rolled_back",
        target: { librarySkillId: "removed-library" },
      }),
    ];
    const onOpenLibrary = vi.fn();
    const onOpenProjects = vi.fn();
    const onOpenGlobal = vi.fn();

    render(
      <SkillsActivityPanel
        onOpenLibrary={onOpenLibrary}
        onOpenProjects={onOpenProjects}
        onOpenGlobal={onOpenGlobal}
      />,
    );

    expect(screen.getByText("skills.activity.batch")).toBeInTheDocument();
    expect(screen.getByText("batch-1")).toBeInTheDocument();
    const rows = screen.getAllByTestId("skills-activity-entry");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("skills.activity.outcome.success");
    expect(rows[1]).toHaveTextContent("skills.activity.outcome.blocked");
    expect(screen.getByText("Careful review")).toBeInTheDocument();
    expect(screen.getByText("removed-library")).toBeInTheDocument();

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: /Careful review/ }));
    expect(onOpenLibrary).toHaveBeenCalledWith("library-1");
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: /Demo workspace/ }));
    expect(onOpenProjects).toHaveBeenCalledWith("workspace-1");
    await userEvent
      .setup()
      .click(
        screen.getByRole("button", { name: /skills.activity.openGlobal/ }),
      );
    expect(onOpenGlobal).toHaveBeenCalledTimes(1);
  });

  it("sends operation/outcome/consumer filters to the paged query", async () => {
    const user = userEvent.setup();
    render(<SkillsActivityPanel />);

    expect(activityQueryMock).toHaveBeenLastCalledWith({ limit: 50 });
    await user.selectOptions(
      screen.getByRole("combobox", {
        name: "skills.activity.operationFilter",
      }),
      "deployment",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "skills.activity.outcomeFilter" }),
      "blocked",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "skills.activity.consumerFilter" }),
      "claude",
    );

    expect(activityQueryMock).toHaveBeenLastCalledWith({
      limit: 50,
      operation: "deployment",
      outcome: "blocked",
      consumer: "claude",
    });
  });

  it("shows pending/error states, refreshes, and loads the next page", async () => {
    activityState.isPending = true;
    const view = render(<SkillsActivityPanel />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "skills.activity.loading",
    );

    activityState.isPending = false;
    activityState.isError = true;
    activityState.error = new Error("raw path /private/secret");
    view.rerender(<SkillsActivityPanel />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.activity.loadError",
    );
    expect(screen.queryByText("/private/secret")).not.toBeInTheDocument();

    activityState.isError = false;
    activityState.hasNextPage = true;
    view.rerender(<SkillsActivityPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.activity.refresh" }),
    );
    await waitFor(() => expect(refetchMock).toHaveBeenCalledTimes(1));
    await user.click(
      screen.getByRole("button", { name: "skills.activity.loadMore" }),
    );
    expect(fetchNextPageMock).toHaveBeenCalledTimes(1);
  });
});
