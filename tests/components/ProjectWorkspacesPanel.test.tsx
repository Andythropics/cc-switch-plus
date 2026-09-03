import { act, screen, render, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectWorkspacesPanel } from "@/components/skills/ProjectWorkspacesPanel";
import type { LibrarySkill } from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

const {
  applyDeploymentsMock,
  pickDirectoryMock,
  registerWorkspaceMock,
  renameWorkspaceMock,
  archiveWorkspaceMock,
  restoreWorkspaceMock,
  relocateWorkspaceMock,
  forgetWorkspaceMock,
  refreshDeploymentsMock,
  claudeState,
  codexState,
  workspaceRows,
  queryState,
  toastSuccessMock,
  toastErrorMock,
} = vi.hoisted(() => ({
  applyDeploymentsMock: vi.fn(),
  pickDirectoryMock: vi.fn(),
  registerWorkspaceMock: vi.fn(),
  renameWorkspaceMock: vi.fn(),
  archiveWorkspaceMock: vi.fn(),
  restoreWorkspaceMock: vi.fn(),
  relocateWorkspaceMock: vi.fn(),
  forgetWorkspaceMock: vi.fn(),
  refreshDeploymentsMock: vi.fn(),
  claudeState: { items: [] as unknown[] },
  codexState: { items: [] as unknown[] },
  workspaceRows: [] as unknown[],
  queryState: {
    projectError: false,
    libraryError: false,
    projectFetching: false,
    deploymentFetching: false,
    applyPending: false,
    recoveryFindings: [] as unknown[],
  },
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

const workspace: ProjectWorkspace = {
  id: "workspace-1",
  displayName: "Demo workspace",
  rootPath: "/tmp/demo-workspace",
  rootKind: "git_repository",
  lifecycle: "active",
  createdAt: 1,
  updatedAt: 1,
};

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

const undeployedLibrarySkill: LibrarySkill = {
  ...librarySkill,
  id: "library-undeployed",
  directory: "unused-skill",
  displayName: "Unused skill",
};
const librarySkills = [librarySkill];

vi.mock("@/hooks/useSkills", () => ({
  useProjectWorkspaces: () => ({
    data: workspaceRows.length ? workspaceRows : [workspace],
    isLoading: false,
    isError: queryState.projectError,
    isFetching: queryState.projectFetching,
  }),
  useRegisterProjectWorkspace: () => ({
    mutateAsync: registerWorkspaceMock,
    isPending: false,
  }),
  useRenameProjectWorkspace: () => ({
    mutateAsync: renameWorkspaceMock,
    isPending: false,
  }),
  useArchiveProjectWorkspace: () => ({
    mutateAsync: archiveWorkspaceMock,
    isPending: false,
  }),
  useRestoreProjectWorkspace: () => ({
    mutateAsync: restoreWorkspaceMock,
    isPending: false,
  }),
  useRelocateProjectWorkspace: () => ({
    mutateAsync: relocateWorkspaceMock,
    isPending: false,
  }),
  useForgetProjectWorkspace: () => ({
    mutateAsync: forgetWorkspaceMock,
    isPending: false,
  }),
  useInspectProjectSkillImports: () => ({
    data: {
      workspaceId: "workspace-1",
      observationToken: "scan-token",
      findings: [],
    },
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  }),
  useApplyProjectSkillImport: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
  useLibrarySkills: () => ({
    data: librarySkills,
    isLoading: false,
    isError: queryState.libraryError,
    isFetching: queryState.projectFetching,
  }),
  useSkillDeployments: ({ consumer }: { consumer: "claude" | "codex" }) => ({
    data: consumer === "claude" ? claudeState : codexState,
    isFetching: queryState.deploymentFetching,
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyDeploymentsMock,
    isPending: queryState.applyPending,
  }),
  useDeploymentRecovery: () => ({
    data: { findings: queryState.recoveryFindings },
    isLoading: false,
    isFetching: false,
    isError: false,
    refetch: vi.fn(),
  }),
  useRefreshSkillDeployments: () => refreshDeploymentsMock,
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: { pickDirectory: pickDirectoryMock },
}));

vi.mock("sonner", () => ({
  toast: { success: toastSuccessMock, error: toastErrorMock },
}));

const archivedWorkspace: ProjectWorkspace = {
  ...workspace,
  id: "workspace-archived",
  displayName: "Archived workspace",
  lifecycle: "archived",
};

const unavailableWorkspace: ProjectWorkspace = {
  ...workspace,
  id: "workspace-unavailable",
  displayName: "Unavailable workspace",
  lifecycle: "unavailable",
};

describe("ProjectWorkspacesPanel", () => {
  it("leaves the Projects view title to the shared Skills header", () => {
    render(<ProjectWorkspacesPanel />);

    expect(
      screen.queryByRole("heading", { name: "skills.projects.title" }),
    ).not.toBeInTheDocument();
  });

  beforeEach(() => {
    pickDirectoryMock.mockReset().mockResolvedValue("/tmp/new-workspace");
    registerWorkspaceMock.mockReset().mockResolvedValue({
      workspace: { ...workspace, id: "workspace-2" },
      scan: {
        selectedPath: "/tmp/new-workspace",
        canonicalRoot: "/tmp/new-workspace",
        rootKind: "non_git",
        scopes: [],
      },
    });
    renameWorkspaceMock.mockReset().mockResolvedValue(workspace);
    archiveWorkspaceMock.mockReset().mockResolvedValue({
      ...workspace,
      lifecycle: "archived",
    });
    restoreWorkspaceMock.mockReset().mockResolvedValue(workspace);
    relocateWorkspaceMock.mockReset().mockResolvedValue({
      outcome: "relocated",
      workspace,
    });
    forgetWorkspaceMock.mockReset().mockResolvedValue(true);
    applyDeploymentsMock.mockReset().mockResolvedValue({
      items: [{ outcome: "applied" }],
    });
    refreshDeploymentsMock.mockReset().mockResolvedValue(undefined);
    toastErrorMock.mockReset();
    toastSuccessMock.mockReset();
    workspaceRows.length = 0;
    librarySkills.length = 0;
    librarySkills.push(librarySkill);
    queryState.projectError = false;
    queryState.libraryError = false;
    queryState.projectFetching = false;
    queryState.deploymentFetching = false;
    queryState.applyPending = false;
    queryState.recoveryFindings = [];
    claudeState.items = [];
    codexState.items = [];
    librarySkill.compatibility.claude = { compatible: true, issues: [] };
    librarySkill.compatibility.codex = { compatible: true, issues: [] };
  });

  it("shows only skills deployed to the selected project workspace", () => {
    librarySkills.push(undeployedLibrarySkill);
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-1" },
        observed: { state: "correct_link" },
      },
    ];

    render(<ProjectWorkspacesPanel />);

    expect(screen.getByText("Careful review")).toBeInTheDocument();
    expect(screen.queryByText("Unused skill")).not.toBeInTheDocument();
  });

  it("keeps Claude and Codex project actions independent at the apply seam", async () => {
    librarySkills.push(undeployedLibrarySkill);
    claudeState.items = [
      {
        librarySkillId: "library-undeployed",
        status: "in_sync",
        desired: { id: "desired-claude" },
        observed: { state: "correct_link" },
      },
    ];
    codexState.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-codex" },
        observed: { state: "correct_link" },
      },
    ];
    render(<ProjectWorkspacesPanel />);
    expect(screen.getByText("skills.recovery.title")).toBeInTheDocument();
    const user = userEvent.setup();
    const deployButtons = screen.getAllByRole("button", {
      name: "skills.projects.deploy",
    });

    await user.click(deployButtons[0]);
    await user.click(deployButtons[1]);

    await waitFor(() => expect(applyDeploymentsMock).toHaveBeenCalledTimes(2));
    expect(applyDeploymentsMock).toHaveBeenNthCalledWith(1, {
      intents: [
        {
          action: "deploy",
          librarySkillId: "library-1",
          target: {
            consumer: "claude",
            workspace: "project",
            workspaceId: "workspace-1",
          },
        },
      ],
    });
    expect(applyDeploymentsMock).toHaveBeenNthCalledWith(2, {
      intents: [
        {
          action: "deploy",
          librarySkillId: "library-undeployed",
          target: {
            consumer: "codex",
            workspace: "project",
            workspaceId: "workspace-1",
          },
        },
      ],
    });
  });

  it("limits pending feedback to the clicked project deployment target", async () => {
    let resolveApply: (() => void) | undefined;
    applyDeploymentsMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveApply = () => resolve({ items: [{ outcome: "applied" }] });
        }),
    );
    codexState.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-codex" },
        observed: { state: "correct_link" },
      },
    ];
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    const skillCard = screen
      .getByText("Careful review")
      .closest("div.rounded-lg");
    expect(skillCard).not.toBeNull();
    const clickedButton = within(skillCard as HTMLElement).getByRole("button", {
      name: "skills.projects.deploy",
    });

    await user.click(clickedButton);

    expect(clickedButton).toBeDisabled();
    expect(
      within(skillCard as HTMLElement).getByRole("button", {
        name: "skills.projects.undeploy",
      }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.projects.register" }),
    ).toBeEnabled();

    await act(async () => resolveApply?.());
    await waitFor(() => expect(clickedButton).toBeEnabled());
  });

  it("blocks Workspace management while recovery confirmation is open", async () => {
    queryState.recoveryFindings = [
      {
        disposition: "recoverable",
        target: {
          consumer: "claude",
          workspace: "project",
          workspaceId: "workspace-1",
        },
        entryName: "review-skill",
        librarySkillId: "library-1",
        libraryDirectory: "review-skill",
        observationToken: "project-recovery-token",
        safeReason: "exact_library_link",
      },
    ];
    const user = userEvent.setup();
    render(<ProjectWorkspacesPanel />);

    await user.click(
      screen.getByRole("checkbox", { name: "skills.recovery.select" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.recovery.reviewSelected",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "skills.projects.register",
          hidden: true,
        }),
      ).toBeDisabled(),
    );
    expect(
      screen.getByRole("button", {
        name: "skills.projects.rename",
        hidden: true,
      }),
    ).toBeDisabled();
  });

  it("Add Skills batches both consumers to the selected stable workspaceId", async () => {
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    );
    expect(
      screen.getByRole("combobox", { name: "skills.batch.target" }),
    ).toBeDisabled();
    for (const checkbox of screen.getAllByRole("checkbox", {
      name: /Careful review skills\.library\.consumer/,
    })) {
      await user.click(checkbox);
    }
    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() => expect(applyDeploymentsMock).toHaveBeenCalledTimes(1));
    expect(applyDeploymentsMock).toHaveBeenCalledWith({
      intents: [
        {
          action: "deploy",
          librarySkillId: "library-1",
          target: {
            consumer: "claude",
            workspace: "project",
            workspaceId: "workspace-1",
          },
        },
        {
          action: "deploy",
          librarySkillId: "library-1",
          target: {
            consumer: "codex",
            workspace: "project",
            workspaceId: "workspace-1",
          },
        },
      ],
    });
    expect(
      applyDeploymentsMock.mock.calls[0][0].intents[0].target,
    ).not.toHaveProperty("path");
  });

  it("limits manual refresh feedback to the refresh control", async () => {
    let resolveRefresh: (() => void) | undefined;
    refreshDeploymentsMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    render(<ProjectWorkspacesPanel />);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "skills.refresh" }));

    expect(refreshDeploymentsMock).toHaveBeenCalledTimes(1);
    const refreshButton = screen.getByRole("button", {
      name: "skills.refresh",
    });
    expect(refreshButton).toHaveAttribute("aria-busy", "true");
    expect(refreshButton).toBeDisabled();
    expect(refreshButton.querySelector("svg")).toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    ).toBeEnabled();

    await act(async () => resolveRefresh?.());
    await waitFor(() =>
      expect(refreshButton).toHaveAttribute("aria-busy", "false"),
    );
  });

  it("keeps background reconciliation from shifting the Projects view", () => {
    queryState.projectFetching = true;
    queryState.deploymentFetching = true;
    codexState.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-codex" },
        observed: { state: "correct_link" },
      },
    ];
    render(<ProjectWorkspacesPanel />);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    const refreshButton = screen.getByRole("button", {
      name: "skills.refresh",
    });
    expect(refreshButton).toBeEnabled();
    expect(refreshButton.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.batch.undeploy" }),
    ).toBeEnabled();
  });

  it("includes Library query failures in the project load alert", () => {
    queryState.libraryError = true;
    render(<ProjectWorkspacesPanel />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.projects.loadError",
    );
  });

  it("aggregates child deployment busy state and clears navigation block on unmount", async () => {
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();
    const view = render(
      <ProjectWorkspacesPanel
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    );
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true);
    });

    view.unmount();
    expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(false);
    expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
  });

  it("blocks local navigation and workspace switching while a child batch is open", async () => {
    render(
      <ProjectWorkspacesPanel onOpenLibrary={vi.fn()} onOpenGlobal={vi.fn()} />,
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.addSkills" }),
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", {
          name: "skills.projects.library",
          hidden: true,
        }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", {
          name: "skills.global.title",
          hidden: true,
        }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", { name: /Demo workspace/, hidden: true }),
      ).toBeDisabled();
    });
    expect(screen.getByTestId("batch-apply")).toBeInTheDocument();
  });

  it("repairs a project drift with the inspection token through the shared resolution controls", async () => {
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-1" },
        observed: { state: "missing" },
        observationToken: "project-observation-1",
      },
    ];
    render(<ProjectWorkspacesPanel />);

    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", { name: "skills.library.repair" })[0],
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "repair",
            librarySkillId: "library-1",
            target: {
              consumer: "claude",
              workspace: "project",
              workspaceId: "workspace-1",
            },
            observationToken: "project-observation-1",
          },
        ],
      }),
    );
  });

  it("disables an incompatible undeployed consumer on a visible project skill", () => {
    librarySkill.compatibility.codex = {
      compatible: false,
      issues: ["Codex does not support this skill"],
    };
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "in_sync",
        desired: { id: "desired-claude" },
        observed: { state: "correct_link" },
      },
    ];
    render(<ProjectWorkspacesPanel />);

    const deployButtons = screen.getAllByRole("button", {
      name: "skills.projects.deploy",
    });
    expect(deployButtons).toHaveLength(1);
    expect(deployButtons[0]).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.projects.undeploy" }),
    ).toBeEnabled();
    expect(
      screen.getByText("Codex does not support this skill"),
    ).toBeInTheDocument();
  });

  it("registers a directory selected through the settings boundary", async () => {
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "skills.projects.register" }),
    );

    await waitFor(() => {
      expect(pickDirectoryMock).toHaveBeenCalledTimes(1);
      expect(registerWorkspaceMock).toHaveBeenCalledWith({
        path: "/tmp/new-workspace",
      });
    });
  });

  it("keeps archived workspaces out of the active list while making them discoverable", async () => {
    workspaceRows.push(workspace, archivedWorkspace);
    render(<ProjectWorkspacesPanel />);

    expect(screen.getByText("Demo workspace")).toBeInTheDocument();
    expect(screen.queryByText("Archived workspace")).not.toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.showArchived" }),
    );
    expect(screen.getByText("Archived workspace")).toBeInTheDocument();
    expect(
      screen.getByText("skills.projects.lifecycle.archived"),
    ).toBeInTheDocument();
  });

  it("renames only the Project Workspace display name", async () => {
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.rename" }),
    );
    const name = screen.getByLabelText("skills.projects.displayName");
    await user.clear(name);
    await user.type(name, "Renamed workspace");
    await user.click(
      screen.getByRole("button", { name: "skills.projects.renameConfirm" }),
    );

    await waitFor(() =>
      expect(renameWorkspaceMock).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        displayName: "Renamed workspace",
      }),
    );
  });

  it("confirms archive with an explicit no-project-mutation promise", async () => {
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.archive" }),
    );

    expect(
      screen.getByText("skills.projects.archiveDescription"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.archiveConfirm" }),
    );

    await waitFor(() =>
      expect(archiveWorkspaceMock).toHaveBeenCalledWith("workspace-1"),
    );
  });

  it("restores an archived workspace through its lifecycle action", async () => {
    workspaceRows.push(archivedWorkspace);
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.showArchived" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.projects.restore" }),
    );

    await waitFor(() =>
      expect(restoreWorkspaceMock).toHaveBeenCalledWith("workspace-archived"),
    );
  });

  it("offers relocation only for unavailable workspaces and sends the picked path", async () => {
    workspaceRows.push(unavailableWorkspace);
    pickDirectoryMock.mockResolvedValueOnce("/tmp/relocated-workspace");
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.relocate" }),
    );
    expect(pickDirectoryMock).toHaveBeenCalledTimes(1);
    await user.click(
      screen.getByRole("button", {
        name: "skills.projects.relocateConfirm",
      }),
    );

    await waitFor(() =>
      expect(relocateWorkspaceMock).toHaveBeenCalledWith({
        workspaceId: "workspace-unavailable",
        path: "/tmp/relocated-workspace",
      }),
    );
  });

  it("explains that an existing old root should be registered distinctly", async () => {
    workspaceRows.push(unavailableWorkspace);
    pickDirectoryMock.mockResolvedValueOnce("/tmp/candidate");
    relocateWorkspaceMock.mockResolvedValueOnce({
      outcome: "registered_distinct",
      workspace: { ...unavailableWorkspace, id: "workspace-distinct" },
    });
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.relocate" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.projects.relocateConfirm",
      }),
    );

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "skills.projects.relocateDistinctSuccess",
      ),
    );
  });

  it("allows permanent Forget only for Archived and surfaces deployment guards", async () => {
    workspaceRows.push(archivedWorkspace);
    forgetWorkspaceMock.mockRejectedValueOnce(
      new Error("deployments remain for this workspace"),
    );
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.showArchived" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.projects.forget" }),
    );
    expect(
      screen.getByText("skills.projects.forgetDescription"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.forgetConfirm" }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "skills.projects.forgetBlocked",
        expect.objectContaining({ description: expect.anything() }),
      ),
    );
    const description = toastErrorMock.mock.calls[0][1].description;
    expect(description.props.children[1].props.details).toBe(
      "deployments remain for this workspace",
    );
    expect(
      screen.getByText("skills.projects.forgetDescription"),
    ).toBeInTheDocument();
  });

  it("keeps lifecycle-blocked actions restricted for visible Archived and Unavailable skills", async () => {
    workspaceRows.push(unavailableWorkspace);
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-lifecycle" },
        observed: { state: "missing" },
        observationToken: "lifecycle-token",
      },
    ];
    render(<ProjectWorkspacesPanel />);

    expect(
      screen.getByRole("button", { name: "skills.projects.deploy" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.projects.undeploy" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "skills.library.repair" }),
    ).not.toBeInTheDocument();

    // Archived rows are hidden from the active list but remain discoverable.
    workspaceRows.length = 0;
    workspaceRows.push(archivedWorkspace);
    // The mock query reads the shared rows on render; remounting gives the
    // lifecycle section a fresh archived-only view.
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", {
        name: "skills.projects.showArchived",
      })[0],
    );
    await user.click(screen.getByText("Archived workspace"));
    const deployButtons = screen.getAllByRole("button", {
      name: "skills.projects.deploy",
    });
    expect(deployButtons).toHaveLength(2);
    deployButtons.forEach((button) => expect(button).toBeDisabled());
    const undeployButtons = screen.getAllByRole("button", {
      name: "skills.projects.undeploy",
    });
    expect(undeployButtons).toHaveLength(2);
    expect(undeployButtons[0]).toBeDisabled();
    expect(undeployButtons[1]).toBeEnabled();
    expect(
      screen.queryAllByRole("button", { name: "skills.library.repair" }),
    ).toHaveLength(0);
  });

  it("keeps Archived cleanup actions reachable even while filesystem actions are blocked", async () => {
    workspaceRows.length = 0;
    workspaceRows.push(archivedWorkspace);
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "archived",
        desired: { id: "desired-archived" },
        observed: { state: "correct_link" },
        observationToken: "archived-token",
      },
    ];
    render(<ProjectWorkspacesPanel />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.showArchived" }),
    );
    await user.click(screen.getByText("Archived workspace"));

    expect(
      screen.getAllByRole("button", { name: "skills.projects.undeploy" })[0],
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.library.forget" }),
    ).toBeEnabled();
  });

  it("keeps DB-only Forget reachable for an Unavailable workspace but blocks Undeploy", () => {
    workspaceRows.length = 0;
    workspaceRows.push(unavailableWorkspace);
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "blocked",
        desired: { id: "desired-unavailable" },
        observed: { state: "missing" },
        observationToken: "unavailable-token",
      },
    ];
    render(<ProjectWorkspacesPanel />);

    expect(
      screen.getAllByRole("button", { name: "skills.projects.undeploy" })[0],
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.forget" }),
    ).toBeEnabled();
  });

  it("keeps Active blocked/incompatible cleanup available", () => {
    workspaceRows.length = 0;
    workspaceRows.push(workspace);
    librarySkill.compatibility.claude = {
      compatible: false,
      issues: ["consumer does not support this skill"],
    };
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "blocked",
        desired: { id: "desired-blocked" },
        observed: { state: "missing" },
        observationToken: "blocked-token",
      },
    ];
    render(<ProjectWorkspacesPanel />);

    expect(
      screen.getAllByRole("button", { name: "skills.projects.undeploy" })[0],
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.library.forget" }),
    ).toBeEnabled();
  });

  it("keeps unsupported cleanup controls disabled and resolution actions hidden", () => {
    workspaceRows.length = 0;
    workspaceRows.push(workspace);
    claudeState.items = [
      {
        librarySkillId: "library-1",
        status: "unsupported",
        desired: { id: "desired-unsupported" },
        observed: { state: "unsupported_platform" },
        observationToken: "unsupported-token",
      },
    ];
    render(<ProjectWorkspacesPanel />);

    expect(
      screen.getAllByRole("button", { name: "skills.projects.undeploy" })[0],
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.library.forget" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "skills.library.repair" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "skills.library.replaceForeignLink",
      }),
    ).not.toBeInTheDocument();
  });
});
