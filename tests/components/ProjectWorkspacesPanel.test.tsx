import { screen, render, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectWorkspacesPanel } from "@/components/skills/ProjectWorkspacesPanel";
import type { LibrarySkill } from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

const {
  applyDeploymentsMock,
  pickDirectoryMock,
  registerWorkspaceMock,
  refreshDeploymentsMock,
  claudeState,
  codexState,
} = vi.hoisted(() => ({
  applyDeploymentsMock: vi.fn(),
  pickDirectoryMock: vi.fn(),
  registerWorkspaceMock: vi.fn(),
  refreshDeploymentsMock: vi.fn(),
  claudeState: { items: [] as unknown[] },
  codexState: { items: [] as unknown[] },
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

vi.mock("@/hooks/useSkills", () => ({
  useProjectWorkspaces: () => ({ data: [workspace], isLoading: false }),
  useRegisterProjectWorkspace: () => ({
    mutateAsync: registerWorkspaceMock,
    isPending: false,
  }),
  useLibrarySkills: () => ({ data: [librarySkill], isLoading: false }),
  useSkillDeployments: ({ consumer }: { consumer: "claude" | "codex" }) => ({
    data: consumer === "claude" ? claudeState : codexState,
  }),
  useApplySkillDeployments: () => ({
    mutateAsync: applyDeploymentsMock,
    isPending: false,
  }),
  useRefreshSkillDeployments: () => refreshDeploymentsMock,
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: { pickDirectory: pickDirectoryMock },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

describe("ProjectWorkspacesPanel", () => {
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
    applyDeploymentsMock.mockReset().mockResolvedValue({
      items: [{ outcome: "applied" }],
    });
    refreshDeploymentsMock.mockReset().mockResolvedValue(undefined);
    claudeState.items = [];
    codexState.items = [];
    librarySkill.compatibility.claude = { compatible: true, issues: [] };
    librarySkill.compatibility.codex = { compatible: true, issues: [] };
  });

  it("keeps Claude and Codex project actions independent at the apply seam", async () => {
    render(<ProjectWorkspacesPanel />);
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
          librarySkillId: "library-1",
          target: {
            consumer: "codex",
            workspace: "project",
            workspaceId: "workspace-1",
          },
        },
      ],
    });
  });

  it("reconciles active deployment observations from the manual refresh control", async () => {
    render(<ProjectWorkspacesPanel />);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "skills.refresh" }));

    expect(refreshDeploymentsMock).toHaveBeenCalledTimes(1);
  });

  it("disables only an incompatible consumer while leaving the other deployable", () => {
    librarySkill.compatibility.codex = {
      compatible: false,
      issues: ["Codex does not support this skill"],
    };
    render(<ProjectWorkspacesPanel />);

    const deployButtons = screen.getAllByRole("button", {
      name: "skills.projects.deploy",
    });
    expect(deployButtons[0]).toBeEnabled();
    expect(deployButtons[1]).toBeDisabled();
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
});
