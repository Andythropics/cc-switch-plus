import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DeploymentRecoveryPanel } from "@/components/skills/DeploymentRecoveryPanel";
import type { DeploymentRecoveryFinding } from "@/lib/api/skills";

const { applyMock, recoveryState } = vi.hoisted(() => ({
  applyMock: vi.fn(),
  recoveryState: {
    findings: [] as DeploymentRecoveryFinding[],
    isLoading: false,
    isFetching: false,
    isError: false,
    refetch: vi.fn(),
    applyPending: false,
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useDeploymentRecovery: () => ({
    data: { findings: recoveryState.findings },
    isLoading: recoveryState.isLoading,
    isFetching: recoveryState.isFetching,
    isError: recoveryState.isError,
    refetch: recoveryState.refetch,
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyMock,
    isPending: recoveryState.applyPending,
  }),
}));

const recoverable = (
  librarySkillId: string,
  consumer: "claude" | "codex",
): DeploymentRecoveryFinding => ({
  disposition: "recoverable",
  target: { consumer, workspace: "global" },
  entryName: `${librarySkillId}-entry`,
  librarySkillId,
  libraryDirectory: `${librarySkillId}-directory`,
  observedTarget: `/private/library/${librarySkillId}`,
  observationToken: `${librarySkillId}-token`,
  safeReason: "exact_library_link",
});

describe("DeploymentRecoveryPanel", () => {
  beforeEach(() => {
    recoveryState.findings = [];
    recoveryState.isLoading = false;
    recoveryState.isFetching = false;
    recoveryState.isError = false;
    recoveryState.applyPending = false;
    recoveryState.refetch.mockReset().mockResolvedValue(undefined);
    applyMock.mockReset().mockResolvedValue({ items: [] });
  });

  it("separates exact recoverable links from rejected observations", () => {
    recoveryState.findings = [
      recoverable("library-1", "claude"),
      {
        disposition: "foreign_link",
        target: { consumer: "codex", workspace: "global" },
        entryName: "foreign-entry",
        observedTarget: "/outside/library",
      },
      {
        disposition: "archived_workspace",
        target: {
          consumer: "claude",
          workspace: "project",
          workspaceId: "workspace-archived",
        },
        entryName: "archived-entry",
        observedTarget: "/private/library/archived",
      },
    ];

    render(<DeploymentRecoveryPanel query={{ workspace: "global" }} />);

    expect(screen.getByText("library-1")).toBeInTheDocument();
    expect(
      screen.getByText("skills.recovery.reason.exact_library_link"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.recovery.disposition.foreign_link"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.recovery.disposition.archived_workspace"),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });

  it("requires item selection and explicit confirmation before ordered recovery", async () => {
    recoveryState.findings = [
      recoverable("library-1", "claude"),
      recoverable("library-2", "codex"),
    ];
    applyMock.mockResolvedValueOnce({
      items: [
        {
          librarySkillId: "library-1",
          target: { consumer: "claude", workspace: "global" },
          outcome: "applied",
        },
        {
          librarySkillId: "library-2",
          target: { consumer: "codex", workspace: "global" },
          outcome: "stale_observation",
        },
      ],
    });
    const user = userEvent.setup();
    render(<DeploymentRecoveryPanel query={{ workspace: "global" }} />);

    const recoverButton = screen.getByRole("button", {
      name: "skills.recovery.reviewSelected",
    });
    expect(recoverButton).toBeDisabled();
    for (const checkbox of screen.getAllByRole("checkbox")) {
      await user.click(checkbox);
    }
    await user.click(recoverButton);
    expect(applyMock).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    );

    await waitFor(() =>
      expect(applyMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "recover",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
            observationToken: "library-1-token",
            confirmed: true,
          },
          {
            action: "recover",
            librarySkillId: "library-2",
            target: { consumer: "codex", workspace: "global" },
            observationToken: "library-2-token",
            confirmed: true,
          },
        ],
      }),
    );

    const results = screen.getAllByTestId(/recovery-result-/);
    expect(results[0]).toHaveTextContent("library-1");
    expect(results[0]).toHaveTextContent("skills.batch.outcome.applied");
    expect(results[1]).toHaveTextContent("library-2");
    expect(results[1]).toHaveTextContent(
      "skills.batch.outcome.stale_observation",
    );
    expect(results[1]).toHaveAttribute("role", "alert");
  });

  it("keeps rejected lifecycle findings read-only and refreshable", async () => {
    recoveryState.findings = [
      {
        disposition: "unavailable_workspace",
        target: {
          consumer: "claude",
          workspace: "project",
          workspaceId: "workspace-unavailable",
        },
        entryName: "workspace-unavailable",
      },
    ];
    const user = userEvent.setup();
    render(
      <DeploymentRecoveryPanel
        query={{ workspace: "project", workspaceId: "workspace-unavailable" }}
      />,
    );

    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.recovery.reviewSelected" }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.refresh" }),
    );
    expect(recoveryState.refetch).toHaveBeenCalledTimes(1);
  });

  it("never selects a rejected finding even when it carries proposal fields", () => {
    recoveryState.findings = [
      {
        disposition: "foreign_link",
        target: { consumer: "claude", workspace: "global" },
        entryName: "spoofed-safe-fields",
        librarySkillId: "library-spoofed",
        observationToken: "spoofed-token",
        safeReason: "exact_library_link",
      },
    ];

    render(<DeploymentRecoveryPanel query={{ workspace: "global" }} />);

    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.recovery.reviewSelected" }),
    ).toBeDisabled();
  });

  it("preserves ordered outcomes while a fresh scan is pending", async () => {
    recoveryState.findings = [recoverable("library-1", "claude")];
    applyMock.mockResolvedValueOnce({
      items: [
        {
          librarySkillId: "library-1",
          target: { consumer: "claude", workspace: "global" },
          outcome: "applied",
        },
      ],
    });
    const user = userEvent.setup();
    const view = render(
      <DeploymentRecoveryPanel query={{ workspace: "global" }} />,
    );
    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.reviewSelected" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    );
    await screen.findByTestId("recovery-result-0");

    recoveryState.isFetching = true;
    view.rerender(<DeploymentRecoveryPanel query={{ workspace: "global" }} />);

    expect(screen.getByTestId("recovery-result-0")).toHaveTextContent(
      "library-1",
    );
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });

  it("reports confirmation as busy and rejects implicit close while apply is pending", async () => {
    recoveryState.findings = [recoverable("library-1", "claude")];
    const onBusyChange = vi.fn();
    const user = userEvent.setup();
    const view = render(
      <DeploymentRecoveryPanel
        query={{ workspace: "global" }}
        onBusyChange={onBusyChange}
      />,
    );
    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.reviewSelected" }),
    );
    await waitFor(() => expect(onBusyChange).toHaveBeenLastCalledWith(true));

    recoveryState.applyPending = true;
    view.rerender(
      <DeploymentRecoveryPanel
        query={{ workspace: "global" }}
        onBusyChange={onBusyChange}
      />,
    );
    await user.keyboard("{Escape}");

    expect(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    ).toBeInTheDocument();
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
  });
});
