import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  WorkspaceSkillCard,
  type WorkspaceDeploymentControl,
} from "@/components/skills/WorkspaceSkillCard";
import type { DeploymentConsumer, LibrarySkill } from "@/lib/api/skills";

const skill: LibrarySkill = {
  id: "review",
  directory: "review",
  displayName: "Review skill",
  description: "Review changes carefully",
  source: { kind: "local_import" },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: "hash",
  acquiredAt: 1,
  updatedAt: 1,
};
function control(consumer: DeploymentConsumer): WorkspaceDeploymentControl {
  const target = {
    consumer,
    workspace: "project" as const,
    workspaceId: "workspace-1",
  };
  return {
    skill,
    target,
    compatible: true,
    deployLabel: `Deploy ${consumer}`,
    undeployLabel: `Undeploy ${consumer}`,
    onApply: vi.fn().mockResolvedValue(undefined),
    deployment: {
      librarySkillId: skill.id,
      libraryDirectory: skill.directory,
      target,
      status: "drift",
      observationToken: `token-${consumer}`,
      desired: {
        id: `desired-${consumer}`,
        librarySkillId: skill.id,
        libraryDirectory: skill.directory,
        target,
        createdAt: 1,
        updatedAt: 1,
      },
      observed: {
        state: "missing",
        targetPath: `/project/${consumer}/review`,
        expectedTarget: "/library/review",
      },
    },
  };
}
describe("WorkspaceSkillCard", () => {
  it("summarizes multiple issues and repairs only the selected consumer from details", async () => {
    const claude = control("claude"),
      codex = control("codex");
    render(<WorkspaceSkillCard skill={skill} controls={[claude, codex]} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "skills.workspaceCard.issueCount",
    );
    expect(
      screen.queryByRole("button", { name: "skills.library.repair" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.library.forget" }),
    ).not.toBeInTheDocument();
    const user = userEvent.setup();
    const opener = screen.getByRole("button", {
      name: "skills.workspaceCard.resolve",
    });
    await user.click(opener);
    const codexPanel = screen.getByRole("region", {
      name: "skills.library.consumerCodex",
    });
    await user.click(
      within(codexPanel).getByRole("button", { name: "skills.library.repair" }),
    );
    expect(codex.onApply).toHaveBeenCalledWith({
      action: "repair",
      librarySkillId: skill.id,
      target: codex.target,
      observationToken: "token-codex",
    });
    expect(claude.onApply).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "common.close" }));
    expect(opener).toHaveFocus();
  });
  it("keeps clear-record confirmation above details and preserves the target", async () => {
    const claude = control("claude");
    render(<WorkspaceSkillCard skill={skill} controls={[claude]} />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.workspaceCard.resolve" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.library.forget" }),
    );
    expect(claude.onApply).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: "skills.library.forgetTitle" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.library.forgetConfirm" }),
    );
    expect(claude.onApply).toHaveBeenCalledWith({
      action: "forget",
      librarySkillId: skill.id,
      target: claude.target,
    });
    expect(
      screen.getByRole("dialog", { name: "skills.workspaceCard.resolve" }),
    ).toBeInTheDocument();
  });
  it("shows completion and returns focus to the card when the issue entry disappears", async () => {
    const claude = control("claude");
    const view = render(
      <WorkspaceSkillCard skill={skill} controls={[claude]} />,
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.workspaceCard.resolve" }),
    );
    const repaired = {
      ...claude,
      deployment: { ...claude.deployment!, status: "in_sync" as const },
    };
    view.rerender(<WorkspaceSkillCard skill={skill} controls={[repaired]} />);
    expect(
      screen.getByText("skills.workspaceCard.resolved"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.close" }));
    expect(
      screen.queryByRole("button", { name: "skills.workspaceCard.resolve" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("article", { name: skill.displayName }),
    ).toHaveFocus();
  });

  it("has only platform toggles on healthy cards", () => {
    const controls = [control("claude"), control("codex")];
    controls.forEach((c) => {
      c.deployment!.status = "in_sync";
      c.deployment!.observed.state = "correct_link";
    });
    render(<WorkspaceSkillCard skill={skill} controls={controls} />);
    expect(screen.getAllByRole("button")).toHaveLength(2);
    expect(
      screen.queryByRole("button", { name: "skills.workspaceCard.resolve" }),
    ).not.toBeInTheDocument();
  });

  it("shows only affected platforms and resolution actions", async () => {
    const claude = control("claude"),
      codex = control("codex");
    codex.deployment!.status = "in_sync";
    render(<WorkspaceSkillCard skill={skill} controls={[claude, codex]} />);
    expect(
      screen.getAllByRole("button", { name: "skills.workspaceCard.resolve" }),
    ).toHaveLength(1);
    await userEvent
      .setup()
      .click(
        screen.getByRole("button", { name: "skills.workspaceCard.resolve" }),
      );
    const dialog = within(screen.getByRole("dialog"));
    expect(
      dialog.getByRole("region", { name: "skills.library.consumerClaude" }),
    ).toBeInTheDocument();
    expect(
      dialog.queryByRole("region", { name: "skills.library.consumerCodex" }),
    ).not.toBeInTheDocument();
    expect(
      dialog.queryByRole("button", { name: /deploy/i }),
    ).not.toBeInTheDocument();
    expect(dialog.queryByText(skill.description!)).not.toBeInTheDocument();
    expect(
      dialog.getByText("skills.workspaceCard.guidance.missing"),
    ).toBeInTheDocument();
  });

  it("prevents closing issues during a mutation", async () => {
    const claude = control("claude");
    const view = render(
      <WorkspaceSkillCard skill={skill} controls={[claude]} />,
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.workspaceCard.resolve" }),
    );
    view.rerender(
      <WorkspaceSkillCard
        skill={skill}
        controls={[{ ...claude, isPending: true }]}
      />,
    );
    expect(screen.getByRole("button", { name: "common.close" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
