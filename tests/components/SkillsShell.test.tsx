import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsShell } from "@/components/skills/SkillsShell";
import type { DeploymentRecoveryFinding } from "@/lib/api/skills";

const { applyMock, recoveryState } = vi.hoisted(() => ({
  applyMock: vi.fn(),
  recoveryState: {
    findings: [] as DeploymentRecoveryFinding[],
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useDeploymentRecovery: () => ({
    data: { findings: recoveryState.findings },
    isError: false,
    isFetching: false,
    isLoading: false,
    refetch: vi.fn(),
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyMock,
    isPending: false,
  }),
}));

const recoverable = (librarySkillId: string): DeploymentRecoveryFinding => ({
  disposition: "recoverable",
  target: { consumer: "claude", workspace: "global" },
  entryName: `${librarySkillId}-entry`,
  librarySkillId,
  libraryDirectory: `${librarySkillId}-directory`,
  observedTarget: `/private/library/${librarySkillId}`,
  observationToken: `${librarySkillId}-token`,
  safeReason: "exact_library_link",
});

const renderShell = () =>
  render(
    <SkillsShell view="skills" onViewChange={vi.fn()}>
      <div>skills content</div>
    </SkillsShell>,
  );

describe("SkillsShell global recovery navigation", () => {
  beforeEach(() => {
    recoveryState.findings = [];
    applyMock.mockReset().mockResolvedValue({ items: [] });
  });

  it("hides the recovery entry without an actionable global candidate", () => {
    recoveryState.findings = [
      {
        ...recoverable("rejected"),
        disposition: "foreign_link",
      },
      {
        ...recoverable("missing-token"),
        observationToken: undefined,
      },
    ];

    renderShell();

    expect(
      screen.queryByTestId("global-recovery-trigger"),
    ).not.toBeInTheDocument();
  });

  it("shows a red wrench and localized tooltip for actionable candidates", async () => {
    recoveryState.findings = [recoverable("library-1")];
    const user = userEvent.setup();
    renderShell();

    const trigger = screen.getByTestId("global-recovery-trigger");
    expect(
      screen.getByRole("navigation", { name: "skills.manage" })
        .lastElementChild,
    ).toBe(trigger.parentElement);
    expect(trigger.querySelector("svg")).toHaveClass("text-destructive");

    await user.hover(trigger);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "skills.recovery.navTooltip",
    );
  });

  it("opens the existing recovery details and confirmation flow", async () => {
    recoveryState.findings = [recoverable("library-1")];
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByTestId("global-recovery-trigger"));
    expect(
      await screen.findByTestId("recovery-group-recoverable"),
    ).toBeInTheDocument();
    expect(screen.getByText("library-1-entry")).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.reviewSelected" }),
    );
    expect(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    );
    await waitFor(() => expect(applyMock).toHaveBeenCalledTimes(1));
  });
});
