import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  projectWorkspacesApi,
  type ProjectSkillImportInspection,
  type ProjectSkillImportIntent,
} from "@/lib/api/projectWorkspaces";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const inspection: ProjectSkillImportInspection = {
  workspaceId: "workspace-1",
  observationToken: "scan-token-1",
  findings: [
    {
      id: "finding-1",
      consumer: "claude",
      scope: "root_level",
      sourcePath: "/tmp/workspace/.claude/skills/review",
      directory: "review",
      validation: { status: "valid", issues: [] },
      compatibility: {
        claude: { compatible: true, issues: [] },
        codex: { compatible: true, issues: [] },
      },
      libraryMatch: { kind: "identical", librarySkillId: "library-1" },
      git: { tracked: false, paths: [] },
      directoryCollision: {
        kind: "none",
        requested: "review",
        suggestions: [],
      },
      replaceEligibility: { eligible: true },
    },
    {
      id: "finding-nested",
      consumer: "codex",
      scope: "nested_unsupported",
      sourcePath: "/tmp/workspace/packages/app/.agents/skills/local",
      directory: "local",
      validation: { status: "valid", issues: [] },
      compatibility: {
        claude: { compatible: true, issues: [] },
        codex: { compatible: true, issues: [] },
      },
      libraryMatch: { kind: "none" },
      git: { tracked: false, paths: [] },
      directoryCollision: {
        kind: "none",
        requested: "local",
        suggestions: [],
      },
      replaceEligibility: {
        eligible: false,
        reason: "nested_unsupported",
      },
    },
  ],
};

describe("Project Skill Import API", () => {
  beforeEach(() => invokeMock.mockReset());

  it("inspects root and nested project Skill findings with a fresh token", async () => {
    invokeMock.mockResolvedValueOnce(inspection);

    const result =
      await projectWorkspacesApi.inspectSkillImports("workspace-1");

    expect(invokeMock).toHaveBeenCalledWith("inspectProjectSkillImports", {
      workspaceId: "workspace-1",
    });
    expect(result.observationToken).toBe("scan-token-1");
    expect(result.findings[1].scope).toBe("nested_unsupported");
  });

  it("applies an explicit import-and-replace resolution without accepting a path", async () => {
    const intent: ProjectSkillImportIntent = {
      workspaceId: "workspace-1",
      findingId: "finding-1",
      observationToken: "scan-token-1",
      mode: "import_and_replace",
      resolution: {
        kind: "replace_library",
        librarySkillId: "library-1",
        confirmed: true,
      },
    };
    invokeMock.mockResolvedValueOnce({
      findingId: "finding-1",
      outcome: "library_replaced",
      librarySkillId: "library-1",
      directory: "review",
    });

    const result = await projectWorkspacesApi.applySkillImport(intent);

    expect(invokeMock).toHaveBeenCalledWith("applyProjectSkillImport", {
      intent,
    });
    expect(result.outcome).toBe("library_replaced");
    expect(invokeMock.mock.calls[0][1].intent).not.toHaveProperty("path");
  });
});
