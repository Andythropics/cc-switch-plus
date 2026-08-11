import { screen, render, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectSkillImportPanel } from "@/components/skills/ProjectSkillImportPanel";
import type { ProjectSkillImportInspection } from "@/lib/api/projectWorkspaces";

const { inspectionState, applySkillImportMock } = vi.hoisted(() => ({
  inspectionState: { value: null as ProjectSkillImportInspection | null },
  applySkillImportMock: vi.fn(),
}));

vi.mock("@/hooks/useSkills", () => ({
  useInspectProjectSkillImports: () => ({
    data: inspectionState.value,
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  }),
  useApplyProjectSkillImport: () => ({
    mutateAsync: applySkillImportMock,
    isPending: false,
  }),
}));

const rootFinding = {
  id: "finding-root",
  consumer: "claude" as const,
  scope: "root_level" as const,
  sourcePath: "/tmp/workspace/.claude/skills/review",
  directory: "review",
  validation: { status: "valid" as const, issues: [] },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  libraryMatch: {
    kind: "identical" as const,
    librarySkillId: "library-1",
    displayName: "Review",
    directory: "review",
  },
  git: { tracked: false, paths: [] },
  directoryCollision: {
    kind: "none" as const,
    requested: "review",
    suggestions: [],
  },
  replaceEligibility: { eligible: true },
};

const nestedFinding = {
  id: "finding-nested",
  consumer: "codex" as const,
  scope: "nested_unsupported" as const,
  sourcePath: "/tmp/workspace/packages/app/.agents/skills/local",
  directory: "local",
  validation: { status: "valid" as const, issues: [] },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  libraryMatch: { kind: "none" as const },
  git: { tracked: false, paths: [] },
  directoryCollision: {
    kind: "none" as const,
    requested: "local",
    suggestions: [],
  },
  replaceEligibility: {
    eligible: false,
    reason: "nested_unsupported" as const,
  },
};

function setInspection(findings: ProjectSkillImportInspection["findings"]) {
  inspectionState.value = {
    workspaceId: "workspace-1",
    observationToken: "scan-token-1",
    findings,
  };
}

describe("ProjectSkillImportPanel", () => {
  beforeEach(() => {
    inspectionState.value = null;
    applySkillImportMock.mockReset().mockResolvedValue({
      findingId: "finding-root",
      outcome: "deployed",
      librarySkillId: "library-1",
      directory: "review",
    });
  });

  it("shows selectable root findings and nested scopes as unsupported", () => {
    setInspection([rootFinding, nestedFinding]);
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);

    expect(screen.getByText("review")).toBeInTheDocument();
    expect(screen.getByText("local")).toBeInTheDocument();
    expect(
      screen.getByText("skills.projects.import.nestedUnsupported"),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });

  it("requires explicit import-and-replace confirmation and sends the scan token", async () => {
    setInspection([rootFinding]);
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("checkbox"));
    expect(
      screen.getByText("skills.projects.import.libraryReuse"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "skills.projects.import.modeImportAndReplace",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    expect(
      screen.getByText("skills.projects.import.replaceDescription"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "skills.projects.import.replaceConfirm",
      }),
    );

    await waitFor(() =>
      expect(applySkillImportMock).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        findingId: "finding-root",
        observationToken: "scan-token-1",
        mode: "import_and_replace",
        resolution: { kind: "reuse", librarySkillId: "library-1" },
      }),
    );
  });

  it("explains tracked content and directory identity mismatch before blocking replacement", () => {
    setInspection([
      {
        ...rootFinding,
        libraryMatch: {
          kind: "different",
          librarySkillId: "library-2",
          displayName: "Other review",
          directory: "review-2",
        },
        git: { tracked: true, paths: [".claude/skills/review/SKILL.md"] },
        directoryCollision: {
          kind: "library",
          requested: "review",
          suggestions: ["review-2"],
        },
        replaceEligibility: {
          eligible: false,
          reason: "git_tracked_content",
        },
      },
    ]);
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    return user.click(screen.getByRole("checkbox")).then(() => {
      expect(
        screen.getByText("skills.projects.import.trackedBlocker"),
      ).toBeInTheDocument();
      expect(
        screen.getByText("skills.projects.import.directoryCollision"),
      ).toBeInTheDocument();
      expect(
        screen.getByText("skills.projects.import.directoryIdentityMismatch"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", {
          name: "skills.projects.import.modeImportAndReplace",
        }),
      ).toBeDisabled();
    });
  });

  it("requires a readable unique directory for Create New and surfaces structured outcomes", async () => {
    setInspection([
      {
        ...rootFinding,
        libraryMatch: { kind: "different" },
        directoryCollision: {
          kind: "library",
          requested: "review",
          suggestions: ["review-2"],
        },
      },
    ]);
    applySkillImportMock.mockResolvedValueOnce({
      findingId: "finding-root",
      outcome: "blocked",
      reason: "invalid_source",
    });
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", {
        name: "skills.projects.import.modeImportOnly",
      }),
    );
    const directory = screen.getByRole("textbox", {
      name: "skills.projects.import.directory",
    });
    expect(directory).toHaveValue("review-2");
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    await waitFor(() =>
      expect(applySkillImportMock).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        findingId: "finding-root",
        observationToken: "scan-token-1",
        mode: "import_only",
        resolution: { kind: "create_new", directory: "review-2" },
      }),
    );
    expect(
      await screen.findByText("skills.projects.import.outcome.blocked"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.projects.import.reason.invalid_source"),
    ).toBeInTheDocument();
  });

  it("lets import-only admit a valid Skill that is incompatible with this consumer", async () => {
    setInspection([
      {
        ...rootFinding,
        libraryMatch: { kind: "none" },
        compatibility: {
          claude: { compatible: false, issues: ["Codex-only metadata"] },
          codex: { compatible: true, issues: [] },
        },
      },
    ]);
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("checkbox"));
    expect(
      screen.getByRole("button", {
        name: "skills.projects.import.modeImportAndReplace",
      }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    await waitFor(() =>
      expect(applySkillImportMock).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        findingId: "finding-root",
        observationToken: "scan-token-1",
        mode: "import_only",
        resolution: { kind: "create_new", directory: "review" },
      }),
    );
  });

  it("offers a unique directory for orphan or reserved collisions without a Library match", async () => {
    setInspection([
      {
        ...rootFinding,
        libraryMatch: { kind: "none" },
        directoryCollision: {
          kind: "reserved",
          requested: "review",
          suggestions: ["review-import"],
        },
      },
    ]);
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("checkbox"));
    const directory = screen.getByRole("textbox", {
      name: "skills.projects.import.directory",
    });
    expect(directory).toHaveValue("review-import");
    await user.clear(directory);
    await user.type(directory, "review");
    expect(
      screen.getByText("skills.projects.import.invalidDirectory"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    ).toBeDisabled();
    await user.clear(directory);
    await user.type(directory, "review-import");
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    await waitFor(() =>
      expect(applySkillImportMock).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        findingId: "finding-root",
        observationToken: "scan-token-1",
        mode: "import_only",
        resolution: { kind: "create_new", directory: "review-import" },
      }),
    );
  });

  it("highlights recovery-required outcomes and preserves the backup path", async () => {
    setInspection([rootFinding]);
    applySkillImportMock.mockResolvedValueOnce({
      findingId: "finding-root",
      outcome: "recovery_required",
      message: "compensation incomplete",
      backupPath: "/tmp/cc-switch-recovery/review",
    });
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    expect(
      await screen.findByText(
        "skills.projects.import.outcome.recovery_required",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.projects.import.recoveryRequired"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("/tmp/cc-switch-recovery/review"),
    ).toBeInTheDocument();
  });

  it("surfaces unexpected apply failures as structured blocked messages", async () => {
    setInspection([rootFinding]);
    applySkillImportMock.mockRejectedValueOnce(new Error("stale observation"));
    render(<ProjectSkillImportPanel workspaceId="workspace-1" />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", { name: "skills.projects.import.submit" }),
    );

    expect(
      await screen.findByText("skills.projects.import.outcome.blocked"),
    ).toBeInTheDocument();
    expect(screen.getByText("stale observation")).toBeInTheDocument();
    expect(
      screen.queryByText("skills.projects.import.reason.invalid_source"),
    ).not.toBeInTheDocument();
  });
});
