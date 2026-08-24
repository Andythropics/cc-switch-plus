import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BatchDeploymentDialog } from "@/components/skills/BatchDeploymentDialog";
import type { DeploymentInspection, LibrarySkill } from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

const makeSkill = (id: string, directory: string): LibrarySkill => ({
  id,
  directory,
  displayName: id,
  description: "",
  source: { kind: "local_import" },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: `${id}-hash`,
  acquiredAt: 1,
  updatedAt: 1,
});

describe("BatchDeploymentDialog", () => {
  it("renders above the fixed application header while keeping the list independently scrollable", () => {
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={Array.from({ length: 12 }, (_, index) =>
          makeSkill(`skill-${index}`, `skill-${index}`),
        )}
        onApply={vi.fn().mockResolvedValue({ items: [] })}
      />,
    );

    const dialog = screen.getByRole("dialog");
    const overlay = document.querySelector('[data-state="open"].fixed.inset-0');
    const selection = screen.getByTestId("batch-skill-selection");
    const scrollRegion = selection.parentElement;
    const footer = screen.getByTestId("batch-apply").parentElement;

    expect(dialog).toHaveClass("z-[60]", "max-h-[90vh]");
    expect(overlay).toHaveClass("z-[60]");
    expect(scrollRegion).toHaveClass("min-h-0", "overflow-auto");
    expect(footer).toHaveClass("flex-shrink-0");
    expect(scrollRegion).not.toContainElement(
      screen.getByRole("heading", { name: "skills.batch.title" }),
    );
    expect(scrollRegion).not.toContainElement(
      screen.getByTestId("batch-apply"),
    );
  });

  it("starts deploy with no targets selected and shows the selected count", async () => {
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-a", "a"), makeSkill("skill-b", "b")]}
        onApply={vi.fn().mockResolvedValue({ items: [] })}
      />,
    );

    expect(screen.getAllByRole("checkbox")).toHaveLength(4);
    screen
      .getAllByRole("checkbox")
      .forEach((checkbox) => expect(checkbox).not.toBeChecked());
    expect(screen.getByTestId("batch-selection-count")).toHaveAttribute(
      "data-count",
      "0",
    );
    expect(screen.getByTestId("batch-apply")).toBeDisabled();

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-a skills.library.consumerClaude",
      }),
    );

    expect(screen.getByTestId("batch-selection-count")).toHaveAttribute(
      "data-count",
      "1",
    );
    expect(screen.getByTestId("batch-apply")).toBeEnabled();
  });

  it("emits deterministic skill x consumer intents for a global deploy", async () => {
    const onApply = vi.fn().mockResolvedValue({
      items: [
        {
          librarySkillId: "skill-a",
          target: { consumer: "claude", workspace: "global" },
          outcome: "applied",
        },
      ],
    });
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-a", "a"), makeSkill("skill-b", "b")]}
        onApply={onApply}
      />,
    );

    for (const checkbox of screen.getAllByRole("checkbox")) {
      await user.click(checkbox);
    }
    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() => expect(onApply).toHaveBeenCalledTimes(1));
    expect(onApply.mock.calls[0][0]).toEqual({
      intents: [
        {
          action: "deploy",
          librarySkillId: "skill-a",
          target: { consumer: "claude", workspace: "global" },
        },
        {
          action: "deploy",
          librarySkillId: "skill-a",
          target: { consumer: "codex", workspace: "global" },
        },
        {
          action: "deploy",
          librarySkillId: "skill-b",
          target: { consumer: "claude", workspace: "global" },
        },
        {
          action: "deploy",
          librarySkillId: "skill-b",
          target: { consumer: "codex", workspace: "global" },
        },
      ],
    });
  });

  it("renders every structured item outcome, including partial success", async () => {
    const onApply = vi.fn().mockResolvedValue({
      items: [
        {
          librarySkillId: "skill-a",
          target: { consumer: "claude", workspace: "global" },
          outcome: "applied",
          message: "created",
        },
        {
          librarySkillId: "skill-a",
          target: { consumer: "codex", workspace: "global" },
          outcome: "blocked",
          message: "consumer blocked",
        },
      ],
    });
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-a", "a")]}
        onApply={onApply}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-a skills.library.consumerClaude",
      }),
    );
    await user.click(screen.getByTestId("batch-apply"));

    expect(await screen.findByTestId("batch-results")).toBeInTheDocument();
    expect(screen.getByTestId("batch-result-0")).toHaveTextContent("applied");
    expect(screen.getByTestId("batch-result-1")).toHaveTextContent("blocked");
    const diagnostic = screen.getByText("consumer blocked");
    expect(diagnostic.closest("details")).not.toHaveAttribute("open");
    expect(screen.getAllByText("skills.error.technicalDetails")).toHaveLength(
      2,
    );
  });

  it("shows a localized failure summary before collapsed backend details", async () => {
    const onApply = vi
      .fn()
      .mockRejectedValue(new Error("database path /Users/test/private.db"));
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-a", "a")]}
        onApply={onApply}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-a skills.library.consumerClaude",
      }),
    );
    await user.click(screen.getByTestId("batch-apply"));

    expect(await screen.findByText("skills.batch.applyFailed")).toBeVisible();
    const diagnostic = screen.getByText("database path /Users/test/private.db");
    expect(diagnostic.closest("details")).not.toHaveAttribute("open");
  });

  it("renders recovery_required as a high-visibility typed outcome", async () => {
    const onApply = vi.fn().mockResolvedValue({
      items: [
        {
          librarySkillId: "skill-recovery",
          target: { consumer: "claude", workspace: "global" },
          outcome: "recovery_required",
          message: "preserve backup",
        },
      ],
    });
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-recovery", "recovery")]}
        onApply={onApply}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-recovery skills.library.consumerClaude",
      }),
    );
    await user.click(screen.getByTestId("batch-apply"));

    expect(
      await screen.findByTestId("batch-recovery-required"),
    ).toHaveTextContent("recovery_required");
    expect(screen.getByTestId("batch-result-0")).toHaveAttribute(
      "role",
      "alert",
    );
    expect(screen.getByText("preserve backup")).toBeInTheDocument();
  });

  it("supports explicit undeploy for an archived registered target", async () => {
    const archived: ProjectWorkspace = {
      id: "workspace-archived",
      displayName: "Archived project",
      rootPath: "/tmp/archived",
      rootKind: "git_repository",
      lifecycle: "archived",
      createdAt: 1,
      updatedAt: 1,
    };
    const onApply = vi.fn().mockResolvedValue({ items: [] });
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[makeSkill("skill-a", "a")]}
        projects={[archived]}
        defaultTarget={{ workspace: "project", workspaceId: archived.id }}
        defaultAction="undeploy"
        inspections={[
          {
            librarySkillId: "skill-a",
            libraryDirectory: "a",
            target: {
              consumer: "claude",
              workspace: "project",
              workspaceId: archived.id,
            },
            desired: {
              id: "desired-a-claude",
              librarySkillId: "skill-a",
              libraryDirectory: "a",
              target: {
                consumer: "claude",
                workspace: "project",
                workspaceId: archived.id,
              },
              createdAt: 1,
              updatedAt: 1,
            },
            observed: {
              state: "correct_link",
              targetPath: "/tmp/archived/.claude/skills/a",
              expectedTarget: "/tmp/library/a",
            },
            observationToken: "obs-a-claude",
            status: "in_sync",
          },
          {
            librarySkillId: "skill-a",
            libraryDirectory: "a",
            target: {
              consumer: "codex",
              workspace: "project",
              workspaceId: archived.id,
            },
            desired: {
              id: "desired-a-codex",
              librarySkillId: "skill-a",
              libraryDirectory: "a",
              target: {
                consumer: "codex",
                workspace: "project",
                workspaceId: archived.id,
              },
              createdAt: 1,
              updatedAt: 1,
            },
            observed: {
              state: "correct_link",
              targetPath: "/tmp/archived/.codex/skills/a",
              expectedTarget: "/tmp/library/a",
            },
            observationToken: "obs-a-codex",
            status: "in_sync",
          },
        ]}
        onApply={onApply}
      />,
    );

    await user.click(screen.getByTestId("batch-apply"));
    await waitFor(() => expect(onApply).toHaveBeenCalledTimes(1));
    expect(onApply.mock.calls[0][0].intents).toEqual([
      {
        action: "undeploy",
        librarySkillId: "skill-a",
        target: {
          consumer: "claude",
          workspace: "project",
          workspaceId: archived.id,
        },
      },
      {
        action: "undeploy",
        librarySkillId: "skill-a",
        target: {
          consumer: "codex",
          workspace: "project",
          workspaceId: archived.id,
        },
      },
    ]);
  });

  it("keeps an incompatible desired consumer selectable for undeploy cleanup", async () => {
    const skill = makeSkill("skill-drift", "drift");
    skill.compatibility.claude = {
      compatible: false,
      issues: ["consumer mismatch"],
    };
    const onApply = vi.fn().mockResolvedValue({ items: [] });
    const user = userEvent.setup();
    const inspection: DeploymentInspection = {
      librarySkillId: "skill-drift",
      libraryDirectory: "drift",
      target: { consumer: "claude", workspace: "global" },
      desired: {
        id: "desired-drift-claude",
        librarySkillId: "skill-drift",
        libraryDirectory: "drift",
        target: { consumer: "claude", workspace: "global" },
        createdAt: 1,
        updatedAt: 1,
      },
      observed: {
        state: "correct_link",
        targetPath: "/tmp/.claude/skills/drift",
        expectedTarget: "/tmp/library/drift",
      },
      observationToken: "obs-drift-claude",
      status: "in_sync",
    };
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        defaultAction="undeploy"
        inspections={[inspection]}
        onApply={onApply}
      />,
    );

    const claude = screen.getByRole("checkbox", {
      name: "skill-drift skills.library.consumerClaude",
    });
    expect(claude).not.toBeDisabled();
    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() => expect(onApply).toHaveBeenCalledTimes(1));
    expect(onApply.mock.calls[0][0].intents).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          action: "undeploy",
          librarySkillId: "skill-drift",
          target: { consumer: "claude", workspace: "global" },
        }),
      ]),
    );
  });

  it("does not select unrelated consumers when undeploying a target", async () => {
    const skill = makeSkill("skill-only-claude", "only-claude");
    const onApply = vi.fn().mockResolvedValue({ items: [] });
    const user = userEvent.setup();
    render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        defaultAction="undeploy"
        inspections={[
          {
            librarySkillId: skill.id,
            libraryDirectory: skill.directory,
            target: { consumer: "claude", workspace: "global" },
            desired: {
              id: "desired-only-claude",
              librarySkillId: skill.id,
              libraryDirectory: skill.directory,
              target: { consumer: "claude", workspace: "global" },
              createdAt: 1,
              updatedAt: 1,
            },
            observed: {
              state: "correct_link",
              targetPath: "/tmp/.claude/skills/only-claude",
              expectedTarget: "/tmp/library/only-claude",
            },
            observationToken: "obs-only-claude",
            status: "in_sync",
          },
        ]}
        onApply={onApply}
      />,
    );

    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() => expect(onApply).toHaveBeenCalledTimes(1));
    expect(onApply.mock.calls[0][0].intents).toEqual([
      {
        action: "undeploy",
        librarySkillId: skill.id,
        target: { consumer: "claude", workspace: "global" },
      },
    ]);
  });

  it("adopts desired undeploy links when inspection arrives after opening", async () => {
    const skill = makeSkill("skill-late-inspection", "late-inspection");
    const onApply = vi.fn().mockResolvedValue({ items: [] });
    const user = userEvent.setup();
    const view = render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        defaultAction="undeploy"
        inspections={[]}
        onApply={onApply}
      />,
    );

    const claude = screen.getByRole("checkbox", {
      name: "skill-late-inspection skills.library.consumerClaude",
    });
    expect(claude).not.toBeChecked();

    const inspection: DeploymentInspection = {
      librarySkillId: skill.id,
      libraryDirectory: skill.directory,
      target: { consumer: "claude", workspace: "global" },
      desired: {
        id: "desired-late-claude",
        librarySkillId: skill.id,
        libraryDirectory: skill.directory,
        target: { consumer: "claude", workspace: "global" },
        createdAt: 1,
        updatedAt: 1,
      },
      observed: {
        state: "correct_link",
        targetPath: "/tmp/.claude/skills/late-inspection",
        expectedTarget: "/tmp/library/late-inspection",
      },
      observationToken: "obs-late-claude",
      status: "in_sync",
    };
    view.rerender(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        defaultAction="undeploy"
        inspections={[inspection]}
        onApply={onApply}
      />,
    );

    await waitFor(() => expect(claude).toBeChecked());
    await user.click(claude);
    view.rerender(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        defaultAction="undeploy"
        inspections={[inspection]}
        onApply={onApply}
      />,
    );
    expect(claude).not.toBeChecked();
  });

  it("keeps the dialog open when Escape or backdrop close is attempted while pending", async () => {
    const onOpenChange = vi.fn();
    const onApply = vi.fn(
      () => new Promise<{ items: never[] }>(() => undefined),
    );
    const user = userEvent.setup();
    const view = render(
      <BatchDeploymentDialog
        open
        onOpenChange={onOpenChange}
        skills={[makeSkill("skill-pending", "pending")]}
        onApply={onApply}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-pending skills.library.consumerClaude",
      }),
    );
    await user.click(screen.getByTestId("batch-apply"));
    view.rerender(
      <BatchDeploymentDialog
        open
        isPending
        onOpenChange={onOpenChange}
        skills={[makeSkill("skill-pending", "pending")]}
        onApply={onApply}
      />,
    );
    await user.keyboard("{Escape}");
    const overlay = document.querySelector(
      "[data-radix-dialog-overlay]",
    ) as HTMLElement | null;
    if (overlay) fireEvent.pointerDown(overlay);

    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(screen.getByTestId("batch-apply")).toBeDisabled();
  });

  it("preserves structured batch results when inspections reconcile after apply", async () => {
    const skill = makeSkill("skill-result", "result");
    const onApply = vi.fn().mockResolvedValue({
      items: [
        {
          librarySkillId: skill.id,
          target: { consumer: "claude", workspace: "global" },
          outcome: "applied",
          message: "applied once",
        },
      ],
    });
    const user = userEvent.setup();
    const view = render(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        inspections={[]}
        onApply={onApply}
      />,
    );

    await user.click(
      screen.getByRole("checkbox", {
        name: "skill-result skills.library.consumerClaude",
      }),
    );
    await user.click(screen.getByTestId("batch-apply"));
    expect(await screen.findByTestId("batch-results")).toHaveTextContent(
      "applied once",
    );

    view.rerender(
      <BatchDeploymentDialog
        open
        onOpenChange={vi.fn()}
        skills={[skill]}
        inspections={[
          {
            librarySkillId: skill.id,
            libraryDirectory: skill.directory,
            target: { consumer: "claude", workspace: "global" },
            observed: {
              state: "correct_link",
              targetPath: "/tmp/.claude/skills/result",
              expectedTarget: "/tmp/library/result",
            },
            observationToken: "after-apply",
            status: "in_sync",
          },
        ]}
        onApply={onApply}
      />,
    );

    expect(screen.getByTestId("batch-results")).toHaveTextContent(
      "applied once",
    );
  });
});
