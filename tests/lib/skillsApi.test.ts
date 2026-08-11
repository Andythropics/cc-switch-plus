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
});
