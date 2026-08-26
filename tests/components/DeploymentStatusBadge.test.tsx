import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
import type { DeploymentStatus, ObservedDeployment } from "@/lib/api/skills";

const observed = (state: ObservedDeployment["state"]): ObservedDeployment => ({
  state,
  targetPath: "/tmp/target",
  expectedTarget: "/tmp/library/skill",
  actualTarget: "/tmp/library/skill",
});

const renderBadge = (
  status: DeploymentStatus,
  state?: ObservedDeployment["state"],
) => {
  render(
    <DeploymentStatusBadge
      status={status}
      observed={state ? observed(state) : undefined}
    />,
  );

  return screen.getByTestId(`deployment-status-${status}`);
};

describe("DeploymentStatusBadge", () => {
  it("renders one Deployed badge for the normal in-sync state", () => {
    const badge = renderBadge("in_sync", "correct_link");

    expect(badge).toHaveTextContent("skills.library.deploymentStatus.deployed");
    expect(badge.childElementCount).toBe(1);
  });

  it.each([
    ["not_deployed", "missing"],
    ["drift", "broken_link"],
    ["conflict", "redirected_link"],
  ] as const)(
    "maps %s with %s observed state to one Not deployed badge",
    (status, state) => {
      const badge = renderBadge(status, state);

      expect(badge).toHaveTextContent(
        "skills.library.deploymentStatus.not_deployed",
      );
      expect(badge.childElementCount).toBe(1);
    },
  );

  it.each(["correct_link", "unrecorded_link"] as const)(
    "uses a connected %s observed link as Deployed",
    (state) => {
      const badge = renderBadge("conflict", state);

      expect(badge).toHaveTextContent(
        "skills.library.deploymentStatus.deployed",
      );
    },
  );

  it("does not render desired, observed, or technical path details", () => {
    const badge = renderBadge("drift", "missing");

    expect(badge.textContent).toBe(
      "skills.library.deploymentStatus.not_deployed",
    );
    expect(badge).not.toHaveTextContent("skills.library.observed");
    expect(badge).not.toHaveTextContent("skills.library.observedState.missing");
    expect(badge).not.toHaveTextContent("skills.library.desiredRecorded");
    expect(badge).not.toHaveTextContent("skills.library.desiredNotRecorded");
    expect(badge).not.toHaveTextContent("/tmp/target");
    expect(badge).not.toHaveAttribute("title");
  });
});
