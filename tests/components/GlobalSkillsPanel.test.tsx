import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";

import {
  GlobalSkillsPanel,
  type GlobalSkillsPanelHandle,
} from "@/components/skills/GlobalSkillsPanel";
import type {
  DeploymentInspection,
  DeploymentStatus,
  LibrarySkill,
} from "@/lib/api/skills";

const deployment = (
  librarySkillId: string,
  libraryDirectory: string,
  consumer: "claude" | "codex",
  status: DeploymentStatus = "in_sync",
  hasDesired = true,
): DeploymentInspection => ({
  librarySkillId,
  libraryDirectory,
  target: { consumer, workspace: "global" },
  desired: hasDesired
    ? {
        id: `desired-${librarySkillId}-${consumer}`,
        librarySkillId,
        libraryDirectory,
        target: { consumer, workspace: "global" },
        createdAt: 1,
        updatedAt: 1,
      }
    : undefined,
  observed: {
    state:
      status === "in_sync"
        ? "correct_link"
        : status === "conflict"
          ? "occupied_directory"
          : "missing",
    targetPath: `/global/${libraryDirectory}`,
    expectedTarget: `/library/${libraryDirectory}`,
  },
  observationToken: `${librarySkillId}-${consumer}-token`,
  status,
});

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
    deploymentLoading: false,
    recoveryFindings: [] as unknown[],
    deploymentItems: [] as DeploymentInspection[],
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
  useSkillDeployments: ({ consumer }: { consumer: "claude" | "codex" }) => ({
    data: {
      items: state.deploymentItems.filter(
        (item) => item.target.consumer === consumer,
      ),
    },
    isError: false,
    isLoading: state.deploymentLoading,
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

  it("leaves Global navigation and batch deploy actions to the app chrome", () => {
    render(<GlobalSkillsPanel />);

    for (const label of [
      "skills.global.library",
      "skills.global.projects",
      "skills.global.batchDeploy",
      "skills.global.batchUndeploy",
      "skills.refresh",
    ]) {
      expect(
        screen.queryByRole("button", { name: label }),
      ).not.toBeInTheDocument();
    }
  });

  beforeEach(() => {
    state.libraryError = false;
    state.projectError = false;
    state.refreshing = false;
    state.deploymentLoading = false;
    libraryRefetch.mockReset().mockResolvedValue(undefined);
    projectRefetch.mockReset().mockResolvedValue(undefined);
    importRefetch.mockReset().mockResolvedValue(undefined);
    refreshMock.mockReset().mockResolvedValue(undefined);
    applyMock.mockReset().mockResolvedValue({ items: [] });
    state.recoveryFindings = [];
    state.deploymentItems = [
      deployment("skill-a", "alpha", "claude"),
      deployment("skill-b", "beta", "codex"),
    ];
  });

  it("hides a Library Skill with no global deployment", () => {
    state.deploymentItems = [
      deployment("skill-a", "alpha", "claude"),
      deployment("skill-b", "beta", "codex", "conflict", false),
    ];

    render(<GlobalSkillsPanel />);

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();
  });

  it("waits for global deployment inspections before showing the empty state", () => {
    state.deploymentItems = [];
    state.deploymentLoading = true;

    render(<GlobalSkillsPanel />);

    expect(screen.queryByText("skills.global.empty")).not.toBeInTheDocument();
  });

  it("keeps tracked drift and conflict deployments visible", () => {
    state.deploymentItems = [
      deployment("skill-a", "alpha", "claude", "drift"),
      deployment("skill-b", "beta", "codex", "conflict"),
    ];

    render(<GlobalSkillsPanel />);

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });

  it("keeps Library identity and filters the global decision list", async () => {
    const user = userEvent.setup();
    const panelRef = createRef<GlobalSkillsPanelHandle>();
    render(<GlobalSkillsPanel ref={panelRef} />);

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();

    await user.type(
      screen.getByPlaceholderText("skills.searchPlaceholder"),
      "alpha",
    );

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();

    await act(async () => {
      await panelRef.current?.refresh();
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalledTimes(1));
    expect(libraryRefetch).toHaveBeenCalledTimes(1);
    expect(projectRefetch).toHaveBeenCalledTimes(1);
    expect(importRefetch).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("skills.recovery.title")).not.toBeInTheDocument();
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
      screen.getByTestId("global-deployment-skill-a-codex"),
    ).getByRole("button", { name: "skills.library.deployCodex" });

    await user.click(clickedButton);

    expect(clickedButton).toBeDisabled();
    expect(
      within(screen.getByTestId("global-deployment-skill-a-claude")).getByRole(
        "button",
        { name: "skills.library.undeployClaude" },
      ),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "skills.global.batchDeploy" }),
    ).not.toBeInTheDocument();

    await act(async () => resolveApply?.());
    await waitFor(() => expect(clickedButton).toBeEnabled());
  });

  it("keeps background reconciliation from shifting the Global view", () => {
    state.refreshing = true;
    render(<GlobalSkillsPanel />);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });

  it("reports navigation busy while its batch dialog is open", async () => {
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();
    const panelRef = createRef<GlobalSkillsPanelHandle>();
    render(
      <GlobalSkillsPanel
        ref={panelRef}
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );

    await act(async () => {
      panelRef.current?.openBatchUndeploy();
    });

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true);
    });
    const actionSelect = screen.getByRole("combobox", {
      name: "skills.batch.action",
    });
    expect(actionSelect).toHaveTextContent("skills.batch.undeploy");
    expect(actionSelect).toBeDisabled();
    expect(
      screen.getByRole("combobox", { name: "skills.batch.target" }),
    ).toBeDisabled();
  });

  it("leaves global recovery details to the shared Skills navigation", () => {
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

    render(<GlobalSkillsPanel />);

    expect(screen.queryByText("skills.recovery.title")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.recovery.reviewSelected" }),
    ).not.toBeInTheDocument();
  });
});
