import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { GlobalSkillsPanel } from "@/components/skills/GlobalSkillsPanel";
import type { LibrarySkill } from "@/lib/api/skills";

const {
  applyMock,
  libraryRefetch,
  projectRefetch,
  importRefetch,
  refreshMock,
  state,
} = vi.hoisted(() => ({
  applyMock: vi.fn(),
  libraryRefetch: vi.fn(),
  projectRefetch: vi.fn(),
  importRefetch: vi.fn(),
  refreshMock: vi.fn(),
  state: {
    libraryError: false,
    projectError: false,
    refreshing: false,
    recoveryFindings: [] as unknown[],
  },
}));

const skills: LibrarySkill[] = [
  {
    id: "skill-a",
    directory: "alpha",
    displayName: "Alpha",
    description: "First skill",
    source: { kind: "local_import" },
    compatibility: {
      claude: { compatible: true, issues: [] },
      codex: { compatible: true, issues: [] },
    },
    contentHash: "a",
    acquiredAt: 1,
    updatedAt: 1,
  },
  {
    id: "skill-b",
    directory: "beta",
    displayName: "Beta",
    description: "Second skill",
    source: { kind: "local_import" },
    compatibility: {
      claude: { compatible: false, issues: ["unsupported"] },
      codex: { compatible: true, issues: [] },
    },
    contentHash: "b",
    acquiredAt: 1,
    updatedAt: 1,
  },
];

vi.mock("@/hooks/useSkills", () => ({
  useLibrarySkills: () => ({
    data: skills,
    isLoading: false,
    isError: state.libraryError,
    isFetching: state.refreshing,
    refetch: libraryRefetch,
  }),
  useProjectWorkspaces: () => ({
    data: [],
    isLoading: false,
    isError: state.projectError,
    isFetching: state.refreshing,
    refetch: projectRefetch,
  }),
  useSkillDeployments: () => ({
    data: { items: [] },
    isError: false,
    isFetching: state.refreshing,
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyMock,
    isPending: false,
  }),
  useInspectGlobalSkillImports: () => ({
    data: { observationToken: "empty", findings: [] },
    isLoading: false,
    isError: false,
    isFetching: state.refreshing,
    refetch: importRefetch,
  }),
  useApplyGlobalSkillImport: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
  useDeploymentRecovery: () => ({
    data: { findings: state.recoveryFindings },
    isLoading: false,
    isFetching: false,
    isError: false,
    refetch: vi.fn(),
  }),
  useRefreshSkillDeployments: () => refreshMock,
}));

describe("GlobalSkillsPanel", () => {
  it("leaves the Global view title to the shared Skills header", () => {
    render(<GlobalSkillsPanel />);

    expect(
      screen.queryByRole("heading", { name: "skills.global.title" }),
    ).not.toBeInTheDocument();
  });

  beforeEach(() => {
    state.libraryError = false;
    state.projectError = false;
    state.refreshing = false;
    libraryRefetch.mockReset().mockResolvedValue(undefined);
    projectRefetch.mockReset().mockResolvedValue(undefined);
    importRefetch.mockReset().mockResolvedValue(undefined);
    refreshMock.mockReset().mockResolvedValue(undefined);
    applyMock.mockReset().mockResolvedValue({ items: [] });
    state.recoveryFindings = [];
  });

  it("keeps Library identity and filters the global decision list", async () => {
    const user = userEvent.setup();
    render(<GlobalSkillsPanel />);

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();

    await user.type(
      screen.getByPlaceholderText("skills.searchPlaceholder"),
      "alpha",
    );

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "skills.refresh" }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalledTimes(1));
    expect(libraryRefetch).toHaveBeenCalledTimes(1);
    expect(projectRefetch).toHaveBeenCalledTimes(1);
    expect(importRefetch).toHaveBeenCalledTimes(1);
    expect(screen.getByText("skills.recovery.title")).toBeInTheDocument();
  });

  it("surfaces a structured load error instead of silently hiding state", () => {
    state.libraryError = true;
    render(<GlobalSkillsPanel />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.global.loadError",
    );
  });

  it("includes auxiliary Project Workspace failures in the global load alert", () => {
    state.projectError = true;
    render(<GlobalSkillsPanel />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.global.loadError",
    );
  });

  it("limits pending feedback to the clicked global deployment target", async () => {
    let resolveApply: (() => void) | undefined;
    applyMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveApply = () => resolve({ items: [{ outcome: "applied" }] });
        }),
    );
    render(<GlobalSkillsPanel />);
    const user = userEvent.setup();
    const clickedButton = within(
      screen.getByTestId("global-deployment-skill-a-claude"),
    ).getByRole("button", { name: "skills.library.deployClaude" });

    await user.click(clickedButton);

    expect(clickedButton).toBeDisabled();
    expect(
      within(screen.getByTestId("global-deployment-skill-a-codex")).getByRole(
        "button",
        { name: "skills.library.deployCodex" },
      ),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.global.batchDeploy" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "skills.refresh" }),
    ).toBeEnabled();

    await act(async () => resolveApply?.());
    await waitFor(() => expect(clickedButton).toBeEnabled());
  });

  it("keeps background reconciliation from shifting the Global view", () => {
    state.refreshing = true;
    render(<GlobalSkillsPanel />);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    const refreshButton = screen.getByRole("button", {
      name: "skills.refresh",
    });
    expect(refreshButton).toBeEnabled();
    expect(refreshButton.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.global.batchDeploy" }),
    ).toBeEnabled();
  });

  it("limits manual refresh feedback to the refresh control", async () => {
    let resolveRefresh: (() => void) | undefined;
    refreshMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    render(<GlobalSkillsPanel />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "skills.refresh" }));

    const refreshButton = screen.getByRole("button", {
      name: "skills.refresh",
    });
    expect(refreshButton).toHaveAttribute("aria-busy", "true");
    expect(refreshButton).toBeDisabled();
    expect(refreshButton.querySelector("svg")).toHaveClass("animate-spin");
    expect(
      screen.getByRole("button", { name: "skills.global.batchDeploy" }),
    ).toBeEnabled();

    await act(async () => resolveRefresh?.());
    await waitFor(() =>
      expect(refreshButton).toHaveAttribute("aria-busy", "false"),
    );
  });

  it("reports navigation busy while its batch dialog is open", async () => {
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();
    const onOpenLibrary = vi.fn();
    const onOpenProjects = vi.fn();
    const user = userEvent.setup();
    render(
      <GlobalSkillsPanel
        onOpenLibrary={onOpenLibrary}
        onOpenProjects={onOpenProjects}
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.global.batchDeploy" }),
    );

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true);
    });
    expect(
      screen.getByRole("button", {
        name: "skills.global.library",
        hidden: true,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "skills.global.projects",
        hidden: true,
      }),
    ).toBeDisabled();
  });

  it("blocks parent navigation while recovery confirmation is open", async () => {
    state.recoveryFindings = [
      {
        disposition: "recoverable",
        target: { consumer: "claude", workspace: "global" },
        entryName: "alpha",
        librarySkillId: "skill-a",
        libraryDirectory: "alpha",
        observationToken: "alpha-token",
        safeReason: "exact_library_link",
      },
    ];
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();
    const user = userEvent.setup();
    render(
      <GlobalSkillsPanel
        onOpenLibrary={vi.fn()}
        onOpenProjects={vi.fn()}
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skills.recovery.select",
      }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.recovery.reviewSelected",
      }),
    );

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true);
    });
    expect(
      screen.getByRole("button", {
        name: "skills.global.library",
        hidden: true,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "skills.global.projects",
        hidden: true,
      }),
    ).toBeDisabled();
  });
});
