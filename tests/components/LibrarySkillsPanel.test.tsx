import { createRef } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
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
  toastErrorMock,
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
  toastErrorMock: vi.fn(),
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
  useLibrarySkills: () => ({ data: [librarySkill], isLoading: false }),
  useUpdateLibrarySkillMetadata: () => ({
    mutateAsync: updateMetadataMock,
    isPending: false,
  }),
  useAcquireLibrarySkillsFromZip: () => ({
    mutateAsync: acquireZipMock,
    isPending: false,
  }),
  useSkillDeployments: () => ({ data: deploymentStateMock }),
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
  toast: { success: vi.fn(), error: toastErrorMock },
}));

describe("LibrarySkillsPanel", () => {
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
    toastErrorMock.mockReset();
    deploymentStateMock.items = [];
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

  it("deploys an acquired Library Skill to Codex Global through the apply seam", async () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
  });

  it("repairs drift using the current inspection observation token", async () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-1" },
        observed: {
          state: "missing",
          targetPath: "/home/me/.claude/skills/review-skill",
          expectedTarget: "/library/review-skill",
        },
        observationToken: "observation-1",
      },
    ];
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

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
            target: { consumer: "claude", workspace: "global" },
            observationToken: "observation-1",
          },
        ],
      }),
    );
  });

  it("requires a distinct confirmation before replacing a foreign link", async () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "conflict",
        desired: { id: "desired-1" },
        observed: {
          state: "redirected_link",
          targetPath: "/home/me/.claude/skills/review-skill",
          expectedTarget: "/library/review-skill",
          actualTarget: "/other/source",
        },
        observationToken: "observation-foreign",
      },
    ];
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", {
        name: "skills.library.replaceForeignLink",
      })[0],
    );

    expect(applyDeploymentsMock).not.toHaveBeenCalled();
    expect(
      screen.getByText("skills.library.replaceForeignLinkDescription"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.replaceForeignLinkConfirm",
      }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "replaceForeignLink",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
            observationToken: "observation-foreign",
            confirmed: true,
          },
        ],
      }),
    );
  });

  it("offers the same confirmed replacement flow for a broken foreign symlink", async () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-1" },
        observed: {
          state: "broken_link",
          targetPath: "/home/me/.claude/skills/review-skill",
          expectedTarget: "/library/review-skill",
          actualTarget: "/removed/source",
        },
        observationToken: "observation-broken",
      },
    ];
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", {
        name: "skills.library.replaceForeignLink",
      })[0],
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.library.replaceForeignLinkConfirm",
      }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "replaceForeignLink",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
            observationToken: "observation-broken",
            confirmed: true,
          },
        ],
      }),
    );
  });

  it("keeps drift Undeploy safe and surfaces the structured outcome", async () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-1" },
        observed: {
          state: "missing",
          targetPath: "/home/me/.claude/skills/review-skill",
          expectedTarget: "/library/review-skill",
        },
        observationToken: "observation-1",
      },
    ];
    applyDeploymentsMock.mockResolvedValueOnce({
      items: [
        {
          outcome: "drift",
          message: "managed link is missing",
        },
      ],
    });
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", {
        name: "skills.library.undeployClaude",
      })[0],
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "undeploy",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
          },
        ],
      }),
    );
    expect(toastErrorMock).toHaveBeenCalledWith(
      expect.stringContaining("managed link is missing"),
    );
  });

  it("labels Forget as database accounting only and confirms before sending it", async () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "drift",
        desired: { id: "desired-1" },
        observed: {
          state: "missing",
          targetPath: "/home/me/.claude/skills/review-skill",
          expectedTarget: "/library/review-skill",
        },
        observationToken: "observation-1",
      },
    ];
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", { name: "skills.library.forget" })[0],
    );

    expect(
      screen.getByText("skills.library.forgetDescription"),
    ).toBeInTheDocument();
    expect(applyDeploymentsMock).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "skills.library.forgetConfirm" }),
    );

    await waitFor(() =>
      expect(applyDeploymentsMock).toHaveBeenCalledWith({
        intents: [
          {
            action: "forget",
            librarySkillId: "library-1",
            target: { consumer: "claude", workspace: "global" },
          },
        ],
      }),
    );
  });

  it("keeps cleanup available while blocking repair for archived deployments", () => {
    deploymentStateMock.items = [
      {
        librarySkillId: "library-1",
        status: "archived",
        desired: { id: "desired-1" },
        observed: { state: "missing" },
        observationToken: "observation-archived",
      },
    ];
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(
      screen.queryByRole("button", { name: "skills.library.repair" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "skills.library.replaceForeignLink",
      }),
    ).not.toBeInTheDocument();
    screen
      .getAllByRole("button", { name: "skills.library.forget" })
      .forEach((button) => expect(button).toBeEnabled());
    expect(
      screen.getAllByRole("button", {
        name: "skills.library.undeployClaude",
      })[0],
    ).toBeEnabled();
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
      screen.getByText("Codex does not support this skill"),
    ).toBeInTheDocument();
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
