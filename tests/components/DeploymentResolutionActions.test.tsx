import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DeploymentResolutionActions } from "@/components/skills/DeploymentResolutionActions";
import type { DeploymentInspection, LibrarySkill } from "@/lib/api/skills";

const skill: LibrarySkill = {
  id: "library-1",
  directory: "careful-review",
  displayName: "Careful review",
  description: "",
  source: { kind: "local_import" },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: "hash",
  acquiredAt: 1,
  updatedAt: 1,
};

const deployed: DeploymentInspection = {
  librarySkillId: skill.id,
  libraryDirectory: skill.directory,
  target: { consumer: "codex", workspace: "global" },
  desired: {
    id: "desired-1",
    librarySkillId: skill.id,
    libraryDirectory: skill.directory,
    target: { consumer: "codex", workspace: "global" },
    createdAt: 1,
    updatedAt: 1,
  },
  observed: {
    state: "correct_link",
    targetPath: "/global/codex/careful-review",
    expectedTarget: "/library/careful-review",
  },
  observationToken: "observation-1",
  status: "in_sync",
};

describe("DeploymentResolutionActions", () => {
  it("requires confirmation before a single undeployment", async () => {
    let resolveApply: (() => void) | undefined;
    const onApply = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveApply = resolve;
        }),
    );
    const user = userEvent.setup();
    render(
      <DeploymentResolutionActions
        skill={skill}
        target={deployed.target}
        deployment={deployed}
        compatible
        deployLabel="Deploy now"
        undeployLabel="Undeploy now"
        onApply={onApply}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Undeploy now" }));

    expect(onApply).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Careful review");
    expect(dialog).toHaveTextContent("skills.library.consumerCodex");
    expect(dialog).toHaveTextContent("skills.batch.global");

    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onApply).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Undeploy now" }));
    const confirm = screen.getByRole("button", {
      name: "skills.library.undeployConfirm",
    });
    await user.click(confirm);

    expect(onApply).toHaveBeenCalledWith({
      action: "undeploy",
      librarySkillId: skill.id,
      target: deployed.target,
    });
    expect(confirm).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();

    resolveApply?.();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("keeps a single deployment immediate", async () => {
    const onApply = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <DeploymentResolutionActions
        skill={skill}
        target={{ consumer: "claude", workspace: "global" }}
        compatible
        deployLabel="Deploy now"
        undeployLabel="Undeploy now"
        onApply={onApply}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Deploy now" }));

    expect(onApply).toHaveBeenCalledWith({
      action: "deploy",
      librarySkillId: skill.id,
      target: { consumer: "claude", workspace: "global" },
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("toggles a compact icon from Deploy to Undeploy after inspection changes", async () => {
    const onApply = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const props = {
      skill,
      target: deployed.target,
      compatible: true,
      iconToggle: true,
      deployLabel: "Deploy to Codex",
      undeployLabel: "Undeploy from Codex",
      onApply,
    };
    const { rerender, container } = render(
      <DeploymentResolutionActions {...props} />,
    );
    const deploy = screen.getByRole("button", { name: "Deploy to Codex" });
    expect(deploy).toHaveAttribute("aria-pressed", "false");
    await user.hover(deploy);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Deploy to Codex",
    );
    expect(container).not.toContainElement(screen.getByRole("tooltip"));
    await user.click(deploy);
    expect(onApply).toHaveBeenLastCalledWith({
      action: "deploy",
      librarySkillId: skill.id,
      target: deployed.target,
    });
    rerender(<DeploymentResolutionActions {...props} deployment={deployed} />);
    const undeploy = screen.getByRole("button", {
      name: "Undeploy from Codex",
    });
    expect(undeploy).toHaveAttribute("aria-pressed", "true");
    await user.click(undeploy);
    expect(onApply).toHaveBeenLastCalledWith({
      action: "undeploy",
      librarySkillId: skill.id,
      target: deployed.target,
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("retains confirmation for an abnormal compact deployment", async () => {
    const onApply = vi.fn().mockResolvedValue(undefined);
    render(
      <DeploymentResolutionActions
        iconToggle
        skill={skill}
        target={deployed.target}
        deployment={{ ...deployed, status: "conflict" }}
        compatible
        deployLabel="Deploy"
        undeployLabel="Undeploy"
        onApply={onApply}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Undeploy" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(onApply).not.toHaveBeenCalled();
  });

  it("disables compact deployment for incompatibility or pending work", () => {
    const onApply = vi.fn();
    const props = {
      iconToggle: true,
      skill,
      target: deployed.target,
      compatible: false,
      deployLabel: "Deploy",
      undeployLabel: "Undeploy",
      onApply,
    };
    const { rerender } = render(<DeploymentResolutionActions {...props} />);
    expect(screen.getByRole("button", { name: "Deploy" })).toBeDisabled();
    rerender(
      <DeploymentResolutionActions
        {...props}
        compatible
        isPending
        deployment={deployed}
      />,
    );
    expect(screen.getByRole("button", { name: "Undeploy" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Undeploy" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
  });
});
