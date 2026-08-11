import { beforeEach, describe, expect, it, vi } from "vitest";

import { skillsApi, type DiscoverableSkill } from "@/lib/api/skills";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const skill: DiscoverableSkill = {
  key: "owner/repo:skills/review",
  name: "review",
  description: "Review changes",
  directory: "skills/review",
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "main",
};

describe("Skills Library API", () => {
  beforeEach(() => invokeMock.mockReset());

  it("acquires a discovered Skill without passing a consumer app", async () => {
    await skillsApi.acquireLibrary(skill, "marketplace", "review-2");

    expect(invokeMock).toHaveBeenCalledWith("acquire_library_skill", {
      skill,
      sourceKind: "marketplace",
      directoryName: "review-2",
    });
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("currentApp");
  });

  it("acquires ZIP snapshots with explicit collision names", async () => {
    await skillsApi.acquireLibraryFromZip("/tmp/skills.zip", {
      review: "review-2",
    });

    expect(invokeMock).toHaveBeenCalledWith("acquire_library_skills_from_zip", {
      filePath: "/tmp/skills.zip",
      directoryNames: { review: "review-2" },
    });
  });

  it("updates presentation metadata without changing directory identity", async () => {
    await skillsApi.updateLibraryMetadata(
      "library-id",
      "Careful review",
      "Notes",
    );

    expect(invokeMock).toHaveBeenCalledWith("update_library_skill_metadata", {
      id: "library-id",
      displayName: "Careful review",
      description: "Notes",
    });
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("directory");
  });

  it("inspects and applies Global Claude deployments through the public seam", async () => {
    await skillsApi.inspectDeployments({
      consumer: "claude",
      workspace: "global",
    });
    expect(invokeMock).toHaveBeenLastCalledWith("inspect_skill_deployments", {
      query: { consumer: "claude", workspace: "global" },
    });

    await skillsApi.applyDeployments({
      intents: [
        {
          action: "deploy",
          librarySkillId: "library-id",
          target: { consumer: "claude", workspace: "global" },
        },
      ],
    });
    expect(invokeMock).toHaveBeenLastCalledWith("apply_skill_deployments", {
      batch: {
        intents: [
          {
            action: "deploy",
            librarySkillId: "library-id",
            target: { consumer: "claude", workspace: "global" },
          },
        ],
      },
    });
  });

  it("inspects and applies Global Codex deployments without accepting a path", async () => {
    await skillsApi.inspectDeployments({
      consumer: "codex",
      workspace: "global",
    });
    expect(invokeMock).toHaveBeenLastCalledWith("inspect_skill_deployments", {
      query: { consumer: "codex", workspace: "global" },
    });

    await skillsApi.applyDeployments({
      intents: [
        {
          action: "deploy",
          librarySkillId: "library-id",
          target: { consumer: "codex", workspace: "global" },
        },
      ],
    });
    expect(invokeMock).toHaveBeenLastCalledWith("apply_skill_deployments", {
      batch: {
        intents: [
          {
            action: "deploy",
            librarySkillId: "library-id",
            target: { consumer: "codex", workspace: "global" },
          },
        ],
      },
    });
    expect(invokeMock.mock.calls[1][1]).not.toHaveProperty("path");
  });

  it("carries inspection tokens for repair and confirmed foreign-link replacement", async () => {
    invokeMock.mockResolvedValue({
      items: [
        {
          librarySkillId: "library-id",
          target: { consumer: "claude", workspace: "global" },
          observed: {
            state: "redirected_link",
            targetPath: "/home/me/.claude/skills/review",
            expectedTarget: "/library/review",
            actualTarget: "/other/review",
          },
          observationToken: "observation-123",
          status: "conflict",
        },
      ],
    });

    const inspection = await skillsApi.inspectDeployments({
      consumer: "claude",
      workspace: "global",
    });
    expect(inspection.items[0].observationToken).toBe("observation-123");

    await skillsApi.applyDeployments({
      intents: [
        {
          action: "repair",
          librarySkillId: "library-id",
          target: { consumer: "claude", workspace: "global" },
          observationToken: "observation-123",
        },
        {
          action: "replaceForeignLink",
          librarySkillId: "library-id",
          target: { consumer: "claude", workspace: "global" },
          observationToken: "observation-123",
          confirmed: true,
        },
      ],
    });

    expect(invokeMock).toHaveBeenLastCalledWith("apply_skill_deployments", {
      batch: {
        intents: [
          {
            action: "repair",
            librarySkillId: "library-id",
            target: { consumer: "claude", workspace: "global" },
            observationToken: "observation-123",
          },
          {
            action: "replaceForeignLink",
            librarySkillId: "library-id",
            target: { consumer: "claude", workspace: "global" },
            observationToken: "observation-123",
            confirmed: true,
          },
        ],
      },
    });
  });

  it("checks and stages an upstream Library update without touching live content", async () => {
    const result = {
      librarySkillId: "library-id",
      outcome: "update_available",
      observationToken: "observation-update",
      stageToken: "stage-update",
      recordedContentHash: "recorded",
      liveContentHash: "live",
      stagedContentHash: "staged",
      localModified: true,
      affectedDeployments: [],
    };
    invokeMock.mockResolvedValueOnce(result);

    await expect(
      skillsApi.checkLibrarySkillUpdate("library-id"),
    ).resolves.toEqual(result);
    expect(invokeMock).toHaveBeenCalledWith("checkLibrarySkillUpdate", {
      librarySkillId: "library-id",
    });
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("path");
  });

  it("applies a staged update with explicit local-modification confirmation", async () => {
    await skillsApi.applyLibrarySkillUpdate({
      librarySkillId: "library-id",
      observationToken: "observation-update",
      stageToken: "stage-update",
      confirmLocalModifications: true,
    });

    expect(invokeMock).toHaveBeenCalledWith("applyLibrarySkillUpdate", {
      intent: {
        librarySkillId: "library-id",
        observationToken: "observation-update",
        stageToken: "stage-update",
        confirmLocalModifications: true,
      },
    });
  });

  it("inspects then deletes only with a fresh observation token", async () => {
    await skillsApi.inspectLibrarySkillDeletion("library-id");
    expect(invokeMock).toHaveBeenCalledWith("inspectLibrarySkillDeletion", {
      librarySkillId: "library-id",
    });

    await skillsApi.deleteLibrarySkill({
      librarySkillId: "library-id",
      observationToken: "deletion-observation",
    });
    expect(invokeMock).toHaveBeenLastCalledWith("deleteLibrarySkill", {
      intent: {
        librarySkillId: "library-id",
        observationToken: "deletion-observation",
      },
    });
    expect(invokeMock.mock.calls[1][1]).not.toHaveProperty("forgetTargets");
  });
});
