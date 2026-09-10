import { createRef } from "react";
import { toast } from "sonner";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  LibrarySkillsPanel,
  type LibrarySkillsPanelHandle,
} from "@/components/skills/LibrarySkillsPanel";
import type { LibrarySkill } from "@/lib/api/skills";

vi.mock("@/components/skills/ExternalSkillUpdatesPanel", () => ({
  ExternalSkillUpdatesPanel: () => null,
}));

vi.mock("@/components/skills/AvailableSkillUpdatesDialog", () => ({
  AvailableSkillUpdatesDialog: ({
    open,
    items,
  }: {
    open: boolean;
    items: Array<{ skill: LibrarySkill }>;
  }) =>
    open ? (
      <div role="dialog" aria-label="available-updates">
        {items.map(({ skill }) => (
          <p key={skill.id}>{skill.directory}</p>
        ))}
      </div>
    ) : null,
}));

vi.mock("@/components/skills/LinkSkillSourceDialog", () => ({
  LinkSkillSourceDialog: ({ skill }: { skill: LibrarySkill }) => (
    <div data-testid="link-source-for">{skill.id}</div>
  ),
}));

const {
  updateMetadataMock,
  acquireZipMock,
  openZipMock,
  revealLibraryMock,
  applyDeploymentsMock,
  refreshDeploymentsMock,
  checkLibraryUpdateMock,
  applyLibraryUpdateMock,
  inspectLibraryDeletionMock,
  deleteLibraryMock,
  deploymentStateMock,
  codexDeploymentStateMock,
  projectDeploymentStateMock,
  projectRows,
  refreshState,
  readPendingState,
  queryErrorState,
  libraryRows,
} = vi.hoisted(() => ({
  updateMetadataMock: vi.fn(),
  acquireZipMock: vi.fn(),
  openZipMock: vi.fn(),
  revealLibraryMock: vi.fn().mockResolvedValue(undefined),
  applyDeploymentsMock: vi.fn(),
  refreshDeploymentsMock: vi.fn(),
  checkLibraryUpdateMock: vi.fn(),
  applyLibraryUpdateMock: vi.fn(),
  inspectLibraryDeletionMock: vi.fn(),
  deleteLibraryMock: vi.fn(),
  deploymentStateMock: { items: [] as unknown[] },
  codexDeploymentStateMock: { items: [] as unknown[] },
  projectDeploymentStateMock: { items: [] as unknown[] },
  projectRows: [] as unknown[],
  refreshState: { isFetching: false },
  readPendingState: { checkUpdate: false, inspect: false },
  queryErrorState: { project: false },
  libraryRows: { data: null as LibrarySkill[] | null },
}));

const librarySkill: LibrarySkill = {
  id: "library-1",
  directory: "review-skill",
  displayName: "Careful review",
  description: "Review changes carefully",
  source: {
    kind: "marketplace",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    skillPath: "skills/review",
    marketplace: "skills.sh",
  },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: "abc",
  acquiredAt: 1,
  updatedAt: 1,
};

vi.mock("@/hooks/useSkills", () => ({
  useLibrarySkills: () => ({
    data: libraryRows.data ?? [librarySkill],
    isLoading: false,
    isFetching: refreshState.isFetching,
  }),
  useProjectWorkspaces: () => ({
    data: projectRows,
    isLoading: false,
    isError: queryErrorState.project,
    isFetching: refreshState.isFetching,
  }),
  useUpdateLibrarySkillMetadata: () => ({
    mutateAsync: updateMetadataMock,
    isPending: false,
  }),
  useAcquireLibrarySkillsFromZip: () => ({
    mutateAsync: acquireZipMock,
    isPending: false,
  }),
  useSkillDeployments: ({
    consumer,
    workspace,
  }: { consumer?: string; workspace?: string } = {}) => ({
    data:
      workspace === "project"
        ? projectDeploymentStateMock
        : consumer === "codex"
          ? codexDeploymentStateMock
          : deploymentStateMock,
    isFetching: refreshState.isFetching,
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyDeploymentsMock,
    isPending: false,
  }),
  useCheckLibrarySkillUpdate: () => ({
    mutateAsync: checkLibraryUpdateMock,
    isPending: readPendingState.checkUpdate,
  }),
  useApplyLibrarySkillUpdate: () => ({
    mutateAsync: applyLibraryUpdateMock,
    isPending: false,
  }),
  useInspectLibrarySkillDeletion: () => ({
    mutateAsync: inspectLibraryDeletionMock,
    isPending: readPendingState.inspect,
  }),
  useDeleteLibrarySkill: () => ({
    mutateAsync: deleteLibraryMock,
    isPending: false,
  }),
  useRefreshSkillDeployments: () => refreshDeploymentsMock,
}));

vi.mock("@/lib/api", () => ({
  skillsApi: {
    openZipFileDialog: openZipMock,
    revealLibrarySkill: revealLibraryMock,
  },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

describe("LibrarySkillsPanel", () => {
  it("keeps manual linking on unlinked cards beside their update action", () => {
    libraryRows.data = [{ ...librarySkill, source: { kind: "local_import" } }];
    render(<LibrarySkillsPanel />);
    const card = screen.getByTestId(`library-skill-summary-${librarySkill.id}`);
    expect(
      within(card).getAllByRole("button", {
        name: "skills.library.update.check",
      }),
    ).toHaveLength(1);
    fireEvent.click(
      within(card).getByRole("button", { name: "skills.external.manual" }),
    );
    expect(screen.getByTestId("link-source-for")).toHaveTextContent(
      librarySkill.id,
    );
    expect(checkLibraryUpdateMock).not.toHaveBeenCalled();
  });

  it("reveals the exact Library Skill from the card icon with a hover label", async () => {
    render(<LibrarySkillsPanel />);
    const card = screen.getByTestId(`library-skill-${librarySkill.id}`);
    const reveal = within(card).getByRole("button", {
      name: "skills.library.reveal",
    });
    expect(reveal).toHaveAttribute("title", "skills.library.reveal");
    expect(reveal.parentElement).toContainElement(
      within(card).getByRole("button", { name: "skills.library.edit" }),
    );
    fireEvent.click(reveal);
    expect(revealLibraryMock).toHaveBeenCalledWith(librarySkill.id);
  });
  it("hides manual association for linked Skills and removes the refresh button", () => {
    render(<LibrarySkillsPanel />);
    expect(
      screen.queryByRole("button", { name: "skills.external.manual" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.refresh" }),
    ).not.toBeInTheDocument();
  });
  it("checks all linked Skills despite search filters and continues after failures", async () => {
    libraryRows.data = [
      librarySkill,
      { ...librarySkill, id: "local", source: { kind: "local_import" } },
      {
        ...librarySkill,
        id: "linked2",
        displayName: "Another",
        source: { kind: "git", repoOwner: "owner", repoName: "repo2" },
      },
    ];
    checkLibraryUpdateMock
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({
        outcome: "up_to_date",
        affectedDeployments: [],
      });
    render(<LibrarySkillsPanel />);
    fireEvent.change(screen.getByPlaceholderText("skills.searchPlaceholder"), {
      target: { value: "Careful" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );
    await waitFor(() =>
      expect(checkLibraryUpdateMock).toHaveBeenCalledTimes(2),
    );
    expect(checkLibraryUpdateMock.mock.calls.map(([id]) => id)).toEqual([
      librarySkill.id,
      "linked2",
    ]);
    expect(applyLibraryUpdateMock).not.toHaveBeenCalled();
  });
  it("shows a green up-to-date notification only when all linked checks are current", async () => {
    checkLibraryUpdateMock.mockResolvedValue({
      outcome: "up_to_date",
      affectedDeployments: [],
    });
    render(<LibrarySkillsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        "skills.library.update.allUpToDate",
        expect.objectContaining({
          className: expect.stringContaining("green"),
        }),
      ),
    );
  });
  it("turns available updates into a warning button that opens the update list", async () => {
    checkLibraryUpdateMock.mockResolvedValue({
      outcome: "update_available",
      stageToken: "stage",
      affectedDeployments: [],
    });
    render(<LibrarySkillsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );
    const button = await screen.findByRole("button", {
      name: "skills.library.update.availableCount",
    });
    expect(button).toHaveClass("border-amber-500", "text-amber-700");
    expect(toast.success).not.toHaveBeenCalled();
    fireEvent.click(button);
    expect(
      screen.getByRole("dialog", { name: "available-updates" }),
    ).toHaveTextContent(librarySkill.directory);
    expect(applyLibraryUpdateMock).not.toHaveBeenCalled();
  });
  it("does not claim up to date after a failed check", async () => {
    checkLibraryUpdateMock.mockRejectedValue(new Error("network"));
    render(<LibrarySkillsPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "skills.library.update.checkAllFailed",
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
  });
  it("disables all-Skill checks when no source is linked", () => {
    libraryRows.data = [{ ...librarySkill, source: { kind: "zip" } }];
    render(<LibrarySkillsPanel />);
    expect(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    ).toBeDisabled();
  });
  it("leaves the Library view title to the shared Skills header", () => {
    render(<LibrarySkillsPanel />);

    expect(
      screen.queryByRole("heading", { name: "skills.library.title" }),
    ).not.toBeInTheDocument();
  });

  it("groups search and Library actions in one responsive toolbar", () => {
    render(<LibrarySkillsPanel />);

    const toolbar = screen.getByRole("toolbar");

    expect(toolbar).toContainElement(
      screen.getByPlaceholderText("skills.searchPlaceholder"),
    );
    expect(within(toolbar).getByRole("search")).toBeInTheDocument();
    expect(toolbar).toContainElement(
      screen.getByRole("button", { name: "skills.batch.open" }),
    );
    expect(toolbar).toContainElement(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );
    expect(toolbar).not.toHaveClass("border-b");
    expect(toolbar.nextElementSibling).toHaveClass("pt-3");
  });

  it("truncates long card content without changing the card size", () => {
    render(<LibrarySkillsPanel />);

    const card = screen.getByTestId("library-skill-library-1");
    expect(card).toHaveClass("h-80", "glass-card");
    expect(card).toHaveAttribute("role", "article");
    expect(screen.getByText(librarySkill.displayName)).toHaveClass("truncate");
    expect(screen.getByText(librarySkill.directory)).toHaveClass("truncate");
    expect(screen.getByText("owner/repo")).toHaveClass("truncate");
    expect(screen.getByText("skills/review")).toHaveClass("truncate");
    expect(screen.getByText(librarySkill.description!)).toHaveClass(
      "line-clamp-4",
    );
    expect(screen.getByText(librarySkill.description!)).toHaveAttribute(
      "title",
      librarySkill.description,
    );
  });

  beforeEach(() => {
    updateMetadataMock.mockReset().mockResolvedValue(librarySkill);
    acquireZipMock.mockReset().mockResolvedValue([librarySkill]);
    openZipMock.mockReset().mockResolvedValue("/tmp/skill.zip");
    applyDeploymentsMock.mockReset().mockResolvedValue({
      items: [{ outcome: "applied" }],
    });
    refreshDeploymentsMock.mockReset().mockResolvedValue(undefined);
    libraryRows.data = null;
    checkLibraryUpdateMock.mockReset();
    applyLibraryUpdateMock.mockReset().mockResolvedValue({
      librarySkillId: "library-1",
      outcome: "updated",
      affectedDeployments: [],
    });
    inspectLibraryDeletionMock.mockReset().mockResolvedValue({
      librarySkillId: "library-1",
      observationToken: "delete-observation",
      targets: [],
      blocked: false,
    });
    deleteLibraryMock.mockReset().mockResolvedValue({
      librarySkillId: "library-1",
      outcome: "deleted",
      items: [],
    });
    deploymentStateMock.items = [];
    codexDeploymentStateMock.items = [];
    projectDeploymentStateMock.items = [];
    projectRows.length = 0;
    refreshState.isFetching = false;
    readPendingState.checkUpdate = false;
    readPendingState.inspect = false;
    queryErrorState.project = false;
    librarySkill.directory = "review-skill";
    librarySkill.displayName = "Careful review";
    librarySkill.description = "Review changes carefully";
    librarySkill.source = {
      kind: "marketplace",
      repoOwner: "owner",
      repoName: "repo",
      repoBranch: "main",
      skillPath: "skills/review",
      marketplace: "skills.sh",
    };
    librarySkill.compatibility.claude = { compatible: true, issues: [] };
    librarySkill.compatibility.codex = { compatible: true, issues: [] };
  });

  it("shows immutable identity, source, compatibility, and edits display metadata", async () => {
    render(<LibrarySkillsPanel />);

    expect(screen.getByText("Careful review")).toBeInTheDocument();
    expect(screen.getByText("review-skill")).toBeInTheDocument();
    expect(screen.getByText(/owner\/repo/)).toBeInTheDocument();
    expect(screen.getByText(/skills\/review/)).toBeInTheDocument();
    expect(
      screen.getByText("skills.library.consumerClaude"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.library.consumerCodex"),
    ).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.edit" }),
    );
    const name = screen.getByLabelText("skills.library.displayName");
    await user.clear(name);
    await user.type(name, "Review teammate");
    await user.click(
      screen.getByRole("button", { name: "skills.library.save" }),
    );

    await waitFor(() =>
      expect(updateMetadataMock).toHaveBeenCalledWith({
        id: "library-1",
        displayName: "Review teammate",
        description: "Review changes carefully",
      }),
    );
    expect(updateMetadataMock.mock.calls[0][0]).not.toHaveProperty("directory");
  });

  it("blocks implicit dismissal of dirty metadata but allows explicit cancel", async () => {
    const user = userEvent.setup();
    render(<LibrarySkillsPanel />);

    await user.click(
      screen.getByRole("button", { name: "skills.library.edit" }),
    );
    const name = screen.getByLabelText("skills.library.displayName");
    await user.type(name, " changed");
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("data-close-blocked", "true");

    await user.keyboard("{Escape}");
    const overlay = document.querySelector(
      "[data-state='open'].fixed.inset-0",
    ) as HTMLElement;
    fireEvent.pointerDown(overlay);
    fireEvent.click(overlay);
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updateMetadataMock).not.toHaveBeenCalled();
  });

  it("labels local imports without implying a live source origin", () => {
    librarySkill.source = { kind: "local_import" };
    render(<LibrarySkillsPanel />);

    expect(
      screen.getByText("skills.library.sourceLocalImport"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/owner\/repo/)).not.toBeInTheDocument();
    expect(
      screen.queryByText("skills.library.sourceMarketplace"),
    ).not.toBeInTheDocument();
  });

  it("acquires a ZIP through the Library-only imperative action", async () => {
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} />);

    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });

    expect(acquireZipMock).toHaveBeenCalledWith({
      filePath: "/tmp/skill.zip",
      directoryNames: {},
    });
  });

  it("deploys an acquired Library Skill to Claude Global through the apply seam", async () => {
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "deploy",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
          },
        ],
      }),
    );
  });

  it("shows progress while checking all linked Skills", async () => {
    let resolveRefresh: (() => void) | undefined;
    checkLibraryUpdateMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveRefresh = () =>
            resolve({ outcome: "up_to_date", affectedDeployments: [] });
        }),
    );
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    );

    expect(checkLibraryUpdateMock).toHaveBeenCalledWith(librarySkill.id);
    const refreshButton = screen.getByRole("button", {
      name: "skills.library.update.checkAll",
    });
    expect(refreshButton).toHaveAttribute("aria-busy", "true");
    expect(refreshButton).toBeDisabled();
    expect(refreshButton.querySelector("svg")).toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.batch.open" }),
    ).toBeDisabled();

    await act(async () => resolveRefresh?.());
    await waitFor(() =>
      expect(refreshButton).toHaveAttribute("aria-busy", "false"),
    );
  });

  it("keeps background reconciliation from shifting the Library", () => {
    refreshState.isFetching = true;
    render(<LibrarySkillsPanel />);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    const refreshButton = screen.getByRole("button", {
      name: "skills.library.update.checkAll",
    });
    expect(refreshButton).toHaveAttribute("aria-busy", "false");
    expect(refreshButton).toBeEnabled();
    expect(refreshButton.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.batch.open" }),
    ).toBeEnabled();
  });

  it("leaves Skills navigation to the shared header", () => {
    render(<LibrarySkillsPanel onOpenProjects={vi.fn()} />);

    expect(
      screen.queryByRole("button", { name: "skills.discover" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.projects.title" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.global.title" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.batch.open" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    ).toBeInTheDocument();
  });

  it("surfaces Project Workspace query failures alongside Library state", () => {
    queryErrorState.project = true;
    render(<LibrarySkillsPanel />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.global.loadError",
    );
  });

  it("opens the deployed projects dialog and lists every project consumer link", async () => {
    projectRows.push(
      {
        id: "workspace-alpha",
        displayName: "Alpha project",
        rootPath: "/projects/alpha",
        rootKind: "git_repository",
        lifecycle: "active",
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: "workspace-beta",
        displayName: "Beta project",
        rootPath: "/projects/beta",
        rootKind: "git_worktree",
        lifecycle: "active",
        createdAt: 2,
        updatedAt: 2,
      },
    );
    inspectLibraryDeletionMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      observationToken: "projects-observation",
      blocked: false,
      targets: [
        {
          actionRequired: "remove_expected_link",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: { consumer: "claude", workspace: "global" },
            desired: { id: "global-deployment" },
            observed: {
              state: "correct_link",
              targetPath: "/global/.claude/skills/review-skill",
              expectedTarget: "/library/review-skill",
              actualTarget: "/library/review-skill",
            },
            observationToken: "global-observation",
            status: "in_sync",
          },
        },
        {
          actionRequired: "remove_expected_link",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: {
              consumer: "claude",
              workspace: "project",
              workspaceId: "workspace-alpha",
            },
            desired: { id: "alpha-claude-deployment" },
            observed: {
              state: "correct_link",
              targetPath: "/projects/alpha/.claude/skills/review-skill",
              expectedTarget: "/library/review-skill",
              actualTarget: "/library/review-skill-alpha",
            },
            observationToken: "alpha-observation",
            status: "in_sync",
          },
        },
        {
          actionRequired: "remove_expected_link",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: {
              consumer: "codex",
              workspace: "project",
              workspaceId: "workspace-beta",
            },
            desired: { id: "beta-codex-deployment" },
            observed: {
              state: "correct_link",
              targetPath: "/projects/beta/.agents/skills/review-skill",
              expectedTarget: "/library/review-skill",
              actualTarget: "/library/review-skill-beta",
            },
            observationToken: "beta-observation",
            status: "in_sync",
          },
        },
      ],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.deployedProjects.action",
      }),
    );

    await waitFor(() =>
      expect(inspectLibraryDeletionMock).toHaveBeenCalledWith("library-1"),
    );
    const alphaRow = await screen.findByTestId(
      "deployed-project-row-claude-workspace-alpha",
    );
    const betaRow = screen.getByTestId(
      "deployed-project-row-codex-workspace-beta",
    );
    expect(alphaRow).toHaveTextContent("Alpha project");
    expect(alphaRow).toHaveTextContent("/projects/alpha");
    expect(alphaRow).toHaveTextContent("skills.library.consumerClaude");
    expect(alphaRow).toHaveTextContent(
      "/projects/alpha/.claude/skills/review-skill",
    );
    expect(alphaRow).toHaveTextContent("/library/review-skill-alpha");
    expect(betaRow).toHaveTextContent("Beta project");
    expect(betaRow).toHaveTextContent("/projects/beta");
    expect(betaRow).toHaveTextContent("skills.library.consumerCodex");
    expect(betaRow).toHaveTextContent(
      "/projects/beta/.agents/skills/review-skill",
    );
    expect(
      screen.queryByText("/global/.claude/skills/review-skill"),
    ).not.toBeInTheDocument();
  });

  it("confirms an exact project target undeploy and refreshes the dialog", async () => {
    projectRows.push({
      id: "workspace-alpha",
      displayName: "Alpha project",
      rootPath: "/projects/alpha",
      rootKind: "git_repository",
      lifecycle: "active",
      createdAt: 1,
      updatedAt: 1,
    });
    inspectLibraryDeletionMock
      .mockResolvedValueOnce({
        librarySkillId: "library-1",
        observationToken: "projects-observation",
        blocked: false,
        targets: [
          {
            actionRequired: "remove_expected_link",
            inspection: {
              librarySkillId: "library-1",
              libraryDirectory: "review-skill",
              target: {
                consumer: "codex",
                workspace: "project",
                workspaceId: "workspace-alpha",
              },
              desired: { id: "alpha-codex-deployment" },
              observed: {
                state: "correct_link",
                targetPath: "/projects/alpha/.agents/skills/review-skill",
                expectedTarget: "/library/review-skill",
                actualTarget: "/library/review-skill",
              },
              observationToken: "alpha-observation",
              status: "in_sync",
            },
          },
        ],
      })
      .mockResolvedValueOnce({
        librarySkillId: "library-1",
        observationToken: "projects-refreshed",
        blocked: false,
        targets: [],
      });
    applyDeploymentsMock.mockResolvedValueOnce({
      items: [
        {
          librarySkillId: "library-1",
          target: {
            consumer: "codex",
            workspace: "project",
            workspaceId: "workspace-alpha",
          },
          outcome: "removed",
        },
      ],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.deployedProjects.action",
      }),
    );
    const row = await screen.findByTestId(
      "deployed-project-row-codex-workspace-alpha",
    );
    await user.click(
      within(row).getByRole("button", { name: "skills.projects.undeploy" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.undeployConfirm",
      }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "undeploy",
            librarySkillId: "library-1",
            target: {
              consumer: "codex",
              workspace: "project",
              workspaceId: "workspace-alpha",
            },
          },
        ],
      }),
    );
    expect(refreshDeploymentsMock).toHaveBeenCalledTimes(1);
    expect(inspectLibraryDeletionMock).toHaveBeenCalledTimes(2);
    expect(
      await screen.findByText("skills.library.deployedProjects.empty"),
    ).toBeInTheDocument();
  });

  it("shows an empty state when the skill has no project deployments", async () => {
    inspectLibraryDeletionMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      observationToken: "global-only-observation",
      blocked: false,
      targets: [
        {
          actionRequired: "remove_expected_link",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: { consumer: "claude", workspace: "global" },
            desired: { id: "global-deployment" },
            observed: {
              state: "correct_link",
              targetPath: "/global/.claude/skills/review-skill",
              expectedTarget: "/library/review-skill",
            },
            observationToken: "global-observation",
            status: "in_sync",
          },
        },
      ],
    });
    render(<LibrarySkillsPanel />);

    await userEvent.setup().click(
      screen.getByRole("button", {
        name: "skills.library.deployedProjects.action",
      }),
    );

    expect(
      await screen.findByText("skills.library.deployedProjects.empty"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId(/deployed-project-row-/),
    ).not.toBeInTheDocument();
  });

  it("refreshes batch target inspections so Project drift status and undeploy selection are visible", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const project = {
      id: "workspace-project",
      displayName: "Project target",
      rootPath: "/tmp/project-target",
      rootKind: "git_repository",
      lifecycle: "active",
      createdAt: 1,
      updatedAt: 1,
    };
    projectRows.push(project);
    const view = render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "skills.batch.open" }));
    await user.click(
      screen.getByRole("combobox", { name: "skills.batch.target" }),
    );
    await user.click(screen.getByRole("option", { name: /Project target/ }));

    await user.click(
      screen.getByRole("combobox", { name: "skills.batch.action" }),
    );
    await user.click(
      screen.getByRole("option", { name: "skills.batch.undeploy" }),
    );

    projectDeploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        libraryDirectory: "review-skill",
        target: {
          consumer: "claude",
          workspace: "project",
          workspaceId: project.id,
        },
        desired: { id: "desired-project" },
        observed: { state: "missing" },
        observationToken: "project-token",
        status: "drift",
      },
    ];
    // Simulate the target inspection resolving after the user has already
    // switched the dialog to undeploy. The desired row must be selected
    // without resetting the target or wiping the decision point.
    view.rerender(<LibrarySkillsPanel />);

    expect(
      await screen.findAllByText(
        "skills.library.deploymentStatus.not_deployed",
      ),
    ).not.toHaveLength(0);
    expect(
      screen.getByRole("checkbox", {
        name: "Careful review skills.library.consumerClaude",
      }),
    ).toBeChecked();
  });

  it("toggles an acquired Library Skill in Codex Global through the apply seam", async () => {
    const view = render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.deployCodex" }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "deploy",
            librarySkillId: "library-1",
            target: { consumer: "codex", workspace: "global" },
          },
        ],
      }),
    );
    codexDeploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-1" },
        observed: { state: "correct_link" },
        observationToken: "observation-1",
      },
    ];
    view.rerender(<LibrarySkillsPanel />);

    expect(
      screen.getByRole("button", { name: "skills.library.deployedCodex" }),
    ).toHaveAttribute("aria-pressed", "true");

    applyDeploymentsMock.mockClear();
    await user.click(
      screen.getByRole("button", { name: "skills.library.deployedCodex" }),
    );
    expect(applyDeploymentsMock).toHaveBeenCalledWith({
      intents: [
        {
          action: "undeploy",
          librarySkillId: "library-1",
          target: { consumer: "codex", workspace: "global" },
        },
      ],
    });
  });

  it("limits pending feedback to the clicked global deployment button", async () => {
    let resolveApply: (() => void) | undefined;
    applyDeploymentsMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveApply = () => resolve({ items: [{ outcome: "applied" }] });
        }),
    );
    codexDeploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-1" },
        observed: { state: "correct_link" },
        observationToken: "observation-1",
      },
    ];
    render(<LibrarySkillsPanel />);
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.library.deployedCodex" }),
    );
    const clickedButton = screen.getByRole("button", {
      name: "skills.library.deployedCodex",
    });
    expect(clickedButton).toHaveAttribute("aria-busy", "true");
    expect(clickedButton).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.library.update.checkAll" }),
    ).toBeEnabled();

    await act(async () => resolveApply?.());
    await waitFor(() =>
      expect(clickedButton).toHaveAttribute("aria-busy", "false"),
    );
  });

  it("renders one card content section above the deployment footer", () => {
    render(<LibrarySkillsPanel />);

    const card = screen.getByTestId("library-skill-library-1");
    expect(card.parentElement).toHaveClass("grid-cols-1", "lg:grid-cols-2");
    const footer = screen.getByTestId("library-skill-footer-library-1");
    expect(within(footer).getAllByRole("button")).toHaveLength(3);
    const editButton = within(card).getByRole("button", {
      name: "skills.library.edit",
    });
    const deleteButton = within(card).getByRole("button", {
      name: "skills.library.delete.action",
    });
    expect(editButton.parentElement).toBe(deleteButton.parentElement);
    expect(editButton.parentElement).toHaveClass("ml-auto", "shrink-0");
    const checkButton = within(card).getByRole("button", {
      name: "skills.library.update.check",
    });
    const summary = screen.getByTestId("library-skill-summary-library-1");
    expect(summary).toContainElement(checkButton);
    expect(summary).toHaveClass("mt-auto", "flex-wrap");
    expect(checkButton).toHaveClass("h-7", "shrink-0", "whitespace-nowrap");
    expect(checkButton.parentElement).toHaveClass("ml-auto");
    expect(summary.lastElementChild).toContainElement(checkButton);
    expect(card.querySelectorAll(".border-t")).toHaveLength(1);
    expect(
      within(footer).getByRole("button", {
        name: "skills.library.deployClaude",
      }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(
      within(footer).getByRole("button", {
        name: "skills.library.deployCodex",
      }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(
      within(footer).getByRole("button", {
        name: "skills.library.deployedProjects.action",
      }),
    ).toBeEnabled();
    expect(footer).not.toContainElement(checkButton);
    expect(footer).not.toContainElement(editButton);
    expect(footer).not.toContainElement(deleteButton);
  });

  it("disables an incompatible consumer while leaving Claude usable", () => {
    librarySkill.compatibility.codex = {
      compatible: false,
      issues: ["Codex does not support this skill"],
    };
    render(<LibrarySkillsPanel />);

    expect(
      screen.getByRole("button", { name: "skills.library.deployCodex" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    ).toBeEnabled();
  });

  it("limits update-check pending feedback to the clicked button", async () => {
    let resolveCheck:
      | ((result: {
          librarySkillId: string;
          outcome: "up_to_date";
          observationToken: string;
          recordedContentHash: string;
          localModified: boolean;
          affectedDeployments: never[];
        }) => void)
      | undefined;
    checkLibraryUpdateMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveCheck = resolve;
        }),
    );
    const view = render(<LibrarySkillsPanel />);
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    readPendingState.checkUpdate = true;
    view.rerender(<LibrarySkillsPanel />);

    const checkButton = screen.getByRole("button", {
      name: "skills.library.update.check",
    });
    expect(checkButton).toHaveAttribute("aria-busy", "true");
    expect(checkButton).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.library.delete.action" }),
    ).toBeEnabled();

    readPendingState.checkUpdate = false;
    await act(async () =>
      resolveCheck?.({
        librarySkillId: "library-1",
        outcome: "up_to_date",
        observationToken: "update-observation",
        recordedContentHash: "abc",
        localModified: false,
        affectedDeployments: [],
      }),
    );
  });

  it("limits deletion-inspection pending feedback to the clicked button", async () => {
    let resolveInspection:
      | ((result: {
          librarySkillId: string;
          observationToken: string;
          targets: never[];
          blocked: boolean;
        }) => void)
      | undefined;
    inspectLibraryDeletionMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveInspection = resolve;
        }),
    );
    const view = render(<LibrarySkillsPanel />);
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.library.delete.action" }),
    );
    readPendingState.inspect = true;
    view.rerender(<LibrarySkillsPanel />);

    const deleteButton = screen.getByRole("button", {
      name: "skills.library.delete.action",
    });
    expect(deleteButton).toHaveAttribute("aria-busy", "true");
    expect(deleteButton).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    ).toBeEnabled();

    readPendingState.inspect = false;
    await act(async () =>
      resolveInspection?.({
        librarySkillId: "library-1",
        observationToken: "delete-observation",
        targets: [],
        blocked: false,
      }),
    );
  });

  it("limits deployed-project inspection feedback to the clicked button", async () => {
    let resolveInspection:
      | ((result: {
          librarySkillId: string;
          observationToken: string;
          targets: never[];
          blocked: boolean;
        }) => void)
      | undefined;
    inspectLibraryDeletionMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveInspection = resolve;
        }),
    );
    const view = render(<LibrarySkillsPanel />);
    const user = userEvent.setup();
    const deployedProjectsButton = screen.getByRole("button", {
      name: "skills.library.deployedProjects.action",
    });
    const checkButton = screen.getByRole("button", {
      name: "skills.library.update.check",
    });
    const deployButton = screen.getByRole("button", {
      name: "skills.library.deployClaude",
    });

    await user.click(deployedProjectsButton);
    readPendingState.inspect = true;
    view.rerender(<LibrarySkillsPanel />);

    expect(deployedProjectsButton).toHaveAttribute("aria-busy", "true");
    expect(deployedProjectsButton).toBeDisabled();
    expect(checkButton).toBeEnabled();
    expect(deployButton).toBeEnabled();

    readPendingState.inspect = false;
    await act(async () =>
      resolveInspection?.({
        librarySkillId: "library-1",
        observationToken: "projects-observation",
        targets: [],
        blocked: false,
      }),
    );
  });

  it("requires explicit confirmation before applying a staged update over local edits", async () => {
    checkLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "update_available",
      observationToken: "update-observation",
      stageToken: "update-stage",
      recordedContentHash: "recorded",
      liveContentHash: "edited",
      stagedContentHash: "upstream",
      localModified: true,
      affectedDeployments: [],
    });
    applyLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "blocked",
      reason: "stale_observation",
      message: "Library changed while confirmation was open",
      affectedDeployments: [],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    expect(checkLibraryUpdateMock).toHaveBeenCalledWith("library-1");

    const applyButton = screen.getByRole("button", {
      name: "skills.library.update.apply",
    });
    const checkButton = screen.getByRole("button", {
      name: "skills.library.update.check",
    });
    expect(applyButton.compareDocumentPosition(checkButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    await user.click(applyButton);
    expect(applyLibraryUpdateMock).not.toHaveBeenCalled();
    expect(
      screen.getByRole("checkbox", {
        name: "skills.library.update.confirmLocalModifications",
      }),
    ).not.toBeChecked();

    await user.click(
      screen.getByRole("checkbox", {
        name: "skills.library.update.confirmLocalModifications",
      }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.update.confirmApply",
      }),
    );

    await waitFor(() =>
      expect(applyLibraryUpdateMock).toHaveBeenCalledWith({
        librarySkillId: "library-1",
        observationToken: "update-observation",
        stageToken: "update-stage",
        confirmLocalModifications: true,
      }),
    );
    expect(
      screen.getByText("skills.library.update.reason.stale_observation"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.library.update.apply" }),
    ).not.toBeInTheDocument();
  });

  it("shows archived compatibility regressions and blocks staged replacement", async () => {
    checkLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "update_available",
      observationToken: "update-observation",
      stageToken: "update-stage",
      recordedContentHash: "recorded",
      stagedContentHash: "upstream",
      localModified: false,
      affectedDeployments: [
        {
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: {
              consumer: "claude",
              workspace: "project",
              workspaceId: "workspace-archived",
            },
            observed: {
              state: "correct_link",
              targetPath: "/skills/review-skill",
              expectedTarget: "/library/review-skill",
            },
            observationToken: "deployment-observation",
            status: "archived",
          },
          currentCompatible: true,
          stagedCompatible: false,
        },
        {
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: { consumer: "codex", workspace: "global" },
            observed: {
              state: "correct_link",
              targetPath: "/skills/review-skill",
              expectedTarget: "/library/review-skill",
            },
            observationToken: "deployment-observation-codex",
            status: "drift",
          },
          currentCompatible: true,
          stagedCompatible: false,
        },
      ],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    expect(
      screen.getAllByText("skills.library.update.compatibilityRegression"),
    ).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: "skills.library.update.apply" }),
    ).toBeDisabled();
    expect(applyLibraryUpdateMock).not.toHaveBeenCalled();
  });

  it("removes a consumed staged token after a successful update", async () => {
    checkLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "update_available",
      observationToken: "update-observation",
      stageToken: "update-stage",
      recordedContentHash: "recorded",
      stagedContentHash: "upstream",
      localModified: false,
      affectedDeployments: [],
    });
    applyLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "updated",
      affectedDeployments: [],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.apply" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.update.confirmApply",
      }),
    );

    await waitFor(() =>
      expect(applyLibraryUpdateMock).toHaveBeenCalledWith({
        librarySkillId: "library-1",
        observationToken: "update-observation",
        stageToken: "update-stage",
        confirmLocalModifications: true,
      }),
    );
    expect(
      screen.queryByRole("button", { name: "skills.library.update.apply" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("library-update-status-library-1"),
    ).not.toBeInTheDocument();
  });

  it("keeps a not-updatable check explicit without exposing an Apply action", async () => {
    checkLibraryUpdateMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "not_updatable",
      observationToken: "update-observation",
      recordedContentHash: "recorded",
      localModified: false,
      affectedDeployments: [],
      message: "local import has no upstream source",
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    expect(
      screen.getByTestId("library-update-status-library-1"),
    ).toHaveTextContent("skills.library.update.outcome.not_updatable");
    expect(
      screen.queryByRole("button", { name: "skills.library.update.apply" }),
    ).not.toBeInTheDocument();
  });

  it("surfaces partial deletion results and recovery backup paths", async () => {
    inspectLibraryDeletionMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      observationToken: "delete-observation",
      blocked: true,
      targets: [
        {
          actionRequired: "remove_expected_link",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: { consumer: "claude", workspace: "global" },
            observed: {
              state: "correct_link",
              targetPath: "/skills/review-skill",
              expectedTarget: "/library/review-skill",
            },
            observationToken: "delete-observation",
            status: "in_sync",
          },
        },
        {
          actionRequired: "forget",
          inspection: {
            librarySkillId: "library-1",
            libraryDirectory: "review-skill",
            target: { consumer: "codex", workspace: "global" },
            observed: {
              state: "unreadable",
              targetPath: "/skills/review-skill",
              expectedTarget: "/library/review-skill",
            },
            observationToken: "delete-observation",
            status: "drift",
          },
        },
      ],
    });
    deleteLibraryMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      outcome: "recovery_required",
      backupPath: "/tmp/backup/review-skill",
      message: "one target remained blocked",
      items: [
        {
          librarySkillId: "library-1",
          target: { consumer: "claude", workspace: "global" },
          outcome: "removed",
        },
        {
          librarySkillId: "library-1",
          target: { consumer: "codex", workspace: "global" },
          outcome: "blocked",
          message: "forget required",
        },
      ],
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.delete.action" }),
    );
    expect(
      screen.getByText("skills.library.delete.forgetRequired"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.library.delete.confirm" }),
    );

    await waitFor(() =>
      expect(deleteLibraryMock).toHaveBeenCalledWith({
        librarySkillId: "library-1",
        observationToken: "delete-observation",
      }),
    );
    expect(screen.getByText("/tmp/backup/review-skill")).toBeInTheDocument();
    expect(screen.getByText("one target remained blocked")).toBeInTheDocument();
    expect(screen.getByText("removed")).toBeInTheDocument();
    expect(screen.getByText("blocked")).toBeInTheDocument();
  });

  it("contains long deletion inspection content without pushing the footer out", async () => {
    const longMessage = `deletion failure: ${"very-long-path-segment/".repeat(20)}`;
    inspectLibraryDeletionMock.mockResolvedValueOnce({
      librarySkillId: "library-1",
      observationToken: "delete-observation",
      blocked: true,
      message: longMessage,
      targets: Array.from({ length: 12 }, (_, index) => ({
        actionRequired: "remove_expected_link",
        inspection: {
          librarySkillId: "library-1",
          libraryDirectory: "review-skill",
          target: {
            consumer: index % 2 === 0 ? "claude" : "codex",
            workspace: "project",
            workspaceId: `workspace-${index}`,
          },
          observed: {
            state: "correct_link",
            targetPath: `/skills/${"very-long-target-path/".repeat(8)}${index}`,
            expectedTarget: "/library/review-skill",
          },
          observationToken: "delete-observation",
          status: "in_sync",
        },
      })),
    });
    render(<LibrarySkillsPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.delete.action" }),
    );

    const dialog = screen.getByRole("dialog");
    const content = screen.getByText(longMessage).closest(".min-h-0");
    const cancelButton = screen.getByRole("button", { name: "common.cancel" });
    const footer = cancelButton.parentElement;

    expect(content).toHaveClass(
      "min-h-0",
      "flex-1",
      "overflow-y-auto",
      "px-6",
      "py-5",
      "break-words",
    );
    expect(content).not.toContainElement(cancelButton);
    expect(dialog).toContainElement(cancelButton);
    expect(dialog).toContainElement(
      screen.getByRole("button", { name: "skills.library.delete.confirm" }),
    );
    expect(footer).toHaveClass("flex-shrink-0");
  });

  it("requires an explicit unique directory when ZIP acquisition collides", async () => {
    acquireZipMock
      .mockRejectedValueOnce(
        new Error(
          "LIBRARY_DIRECTORY_CONFLICT: 'review-skill' is already in use",
        ),
      )
      .mockResolvedValueOnce([librarySkill]);
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} />);

    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });

    const user = userEvent.setup();
    const directory = await screen.findByLabelText("skills.library.directory");
    await user.clear(directory);
    await user.type(directory, "review-skill-copy");
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );

    await waitFor(() =>
      expect(acquireZipMock).toHaveBeenLastCalledWith({
        filePath: "/tmp/skill.zip",
        directoryNames: { "review-skill": "review-skill-copy" },
      }),
    );
  });
  it("retains earlier ZIP renames when resolving another collision", async () => {
    acquireZipMock
      .mockRejectedValueOnce(
        new Error("LIBRARY_DIRECTORY_CONFLICT: 'a' is already in use"),
      )
      .mockRejectedValueOnce(
        new Error("LIBRARY_DIRECTORY_CONFLICT: 'b' is already in use"),
      )
      .mockResolvedValueOnce([librarySkill]);
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} />);
    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("skills.library.directory")).toHaveValue(
        "b-2",
      ),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );
    expect(acquireZipMock).toHaveBeenLastCalledWith({
      filePath: "/tmp/skill.zip",
      directoryNames: { a: "a-2", b: "b-2" },
    });
  });

  it("focuses an Activity target after the Library finishes loading", () => {
    libraryRows.data = [];
    const view = render(
      <LibrarySkillsPanel focusLibrarySkillId={librarySkill.id} />,
    );
    libraryRows.data = [librarySkill];
    view.rerender(<LibrarySkillsPanel focusLibrarySkillId={librarySkill.id} />);
    expect(
      screen.getByRole("textbox", { name: "skills.searchPlaceholder" }),
    ).toHaveValue(librarySkill.displayName);
  });

  it("updates the original ZIP mapping when a chosen rename also collides", async () => {
    acquireZipMock
      .mockRejectedValueOnce(
        new Error("LIBRARY_DIRECTORY_CONFLICT: 'a' is already in use"),
      )
      .mockRejectedValueOnce(
        new Error("LIBRARY_DIRECTORY_CONFLICT: 'a-2' is already in use"),
      )
      .mockResolvedValueOnce([librarySkill]);
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} />);
    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );
    const input = await screen.findByLabelText("skills.library.directory");
    await waitFor(() => expect(input).toHaveValue("a-2-2"));
    await user.clear(input);
    await user.type(input, "a-3");
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );
    expect(acquireZipMock).toHaveBeenLastCalledWith({
      filePath: "/tmp/skill.zip",
      directoryNames: { a: "a-3" },
    });
  });
});
