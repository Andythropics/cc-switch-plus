import { createRef } from "react";
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

const {
  updateMetadataMock,
  acquireZipMock,
  openZipMock,
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
  queryErrorState,
} = vi.hoisted(() => ({
  updateMetadataMock: vi.fn(),
  acquireZipMock: vi.fn(),
  openZipMock: vi.fn(),
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
  queryErrorState: { project: false },
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
    data: [librarySkill],
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
    isPending: false,
  }),
  useApplyLibrarySkillUpdate: () => ({
    mutateAsync: applyLibraryUpdateMock,
    isPending: false,
  }),
  useInspectLibrarySkillDeletion: () => ({
    mutateAsync: inspectLibraryDeletionMock,
    isPending: false,
  }),
  useDeleteLibrarySkill: () => ({
    mutateAsync: deleteLibraryMock,
    isPending: false,
  }),
  useRefreshSkillDeployments: () => refreshDeploymentsMock,
}));

vi.mock("@/lib/api", () => ({
  skillsApi: { openZipFileDialog: openZipMock },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

describe("LibrarySkillsPanel", () => {
  it("leaves the Library view title to the shared Skills header", () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(
      screen.queryByRole("heading", { name: "skills.library.title" }),
    ).not.toBeInTheDocument();
  });

  it("groups search and Library actions in one responsive toolbar", () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const toolbar = screen.getByRole("toolbar");

    expect(toolbar).toContainElement(
      screen.getByPlaceholderText("skills.searchPlaceholder"),
    );
    expect(toolbar).toContainElement(
      screen.getByRole("button", { name: "skills.batch.deploy" }),
    );
    expect(toolbar).toContainElement(
      screen.getByRole("button", { name: "skills.refresh" }),
    );
  });

  it("wraps long dynamic Skill metadata without widening the panel", () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(screen.getByText(librarySkill.displayName)).toHaveClass(
      "break-words",
    );
    expect(screen.getByText(librarySkill.directory)).toHaveClass("break-all");
    expect(screen.getByText("owner/repo")).toHaveClass("break-all");
    expect(screen.getByText("skills/review")).toHaveClass("break-all");
  });

  beforeEach(() => {
    updateMetadataMock.mockReset().mockResolvedValue(librarySkill);
    acquireZipMock.mockReset().mockResolvedValue([librarySkill]);
    openZipMock.mockReset().mockResolvedValue("/tmp/skill.zip");
    applyDeploymentsMock.mockReset().mockResolvedValue({
      items: [{ outcome: "applied" }],
    });
    refreshDeploymentsMock.mockReset().mockResolvedValue(undefined);
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
    queryErrorState.project = false;
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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel ref={ref} onOpenDiscovery={vi.fn()} />);

    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });

    expect(acquireZipMock).toHaveBeenCalledWith({
      filePath: "/tmp/skill.zip",
      directoryNames: {},
    });
  });

  it("deploys an acquired Library Skill to Claude Global through the apply seam", async () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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

  it("reconciles active deployment observations from the manual refresh control", async () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "skills.refresh" }));

    expect(refreshDeploymentsMock).toHaveBeenCalledTimes(1);
  });

  it("shows and locks the refresh control while reconciliation is pending", () => {
    refreshState.isFetching = true;
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(screen.getByRole("status")).toHaveTextContent("skills.refreshing");
    expect(
      screen.getByRole("button", { name: "skills.refresh" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.batch.deploy" }),
    ).toBeDisabled();
  });

  it("leaves Skills navigation to the shared header", () => {
    render(
      <LibrarySkillsPanel onOpenDiscovery={vi.fn()} onOpenProjects={vi.fn()} />,
    );

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
      screen.getByRole("button", { name: "skills.batch.deploy" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.refresh" }),
    ).toBeInTheDocument();
  });

  it("surfaces Project Workspace query failures alongside Library state", () => {
    queryErrorState.project = true;
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    const view = render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.batch.deploy" }),
    );
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
    view.rerender(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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

  it("deploys an acquired Library Skill to Codex Global through the apply seam", async () => {
    const view = render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    view.rerender(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(
      screen.getByRole("button", { name: "skills.library.deployedCodex" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("renders responsive cards with upper actions and three footer actions", () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const card = screen.getByTestId("library-skill-library-1");
    expect(card.parentElement).toHaveClass(
      "grid-cols-1",
      "md:grid-cols-2",
      "lg:grid-cols-3",
    );
    const footer = card.querySelector("footer");
    expect(footer).not.toBeNull();
    expect(within(footer as HTMLElement).getAllByRole("button")).toHaveLength(
      3,
    );
    expect(
      within(footer as HTMLElement).getByRole("button", {
        name: "skills.library.deployClaude",
      }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(
      within(footer as HTMLElement).getByRole("button", {
        name: "skills.library.deployCodex",
      }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(
      within(footer as HTMLElement).getByRole("button", {
        name: "skills.library.deployedProjects.action",
      }),
    ).toBeEnabled();
    expect(footer).not.toContainElement(
      within(card).getByRole("button", { name: "skills.library.update.check" }),
    );
    expect(footer).not.toContainElement(
      within(card).getByRole("button", { name: "skills.library.edit" }),
    );
    expect(footer).not.toContainElement(
      within(card).getByRole("button", {
        name: "skills.library.delete.action",
      }),
    );
  });

  it("disables an incompatible consumer while leaving Claude usable", () => {
    librarySkill.compatibility.codex = {
      compatible: false,
      issues: ["Codex does not support this skill"],
    };
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(
      screen.getByRole("button", { name: "skills.library.deployCodex" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.deployClaude" }),
    ).toBeEnabled();
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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.update.check" }),
    );
    expect(checkLibraryUpdateMock).toHaveBeenCalledWith("library-1");

    await user.click(
      screen.getByRole("button", { name: "skills.library.update.apply" }),
    );
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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
    render(<LibrarySkillsPanel ref={ref} onOpenDiscovery={vi.fn()} />);

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
});
