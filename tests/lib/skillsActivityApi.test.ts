import { beforeEach, describe, expect, it, vi } from "vitest";

import { skillsApi, type SkillActivityQuery } from "@/lib/api/skills";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("Skills Activity API", () => {
  beforeEach(() => invokeMock.mockReset());

  it("lists newest activity with typed filters and an opaque cursor", async () => {
    const query: SkillActivityQuery = {
      operation: "deployment",
      reason: "repair",
      outcome: "compensation_failed",
      librarySkillId: "library-skill-1",
      workspaceId: "workspace-1",
      consumer: "claude",
      workspaceKind: "project",
      since: 1_700_000_000_000,
      until: 1_700_100_000_000,
      cursor: { occurredAt: 1_700_050_000_000, id: 9 },
      limit: 25,
    };

    await skillsApi.listActivity(query);

    expect(invokeMock).toHaveBeenCalledWith("listSkillActivity", { query });
  });

  it("keeps an omitted query explicit and does not expose arbitrary payloads", async () => {
    await skillsApi.listActivity();

    expect(invokeMock).toHaveBeenCalledWith("listSkillActivity", {
      query: null,
    });
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("payload");
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("path");
    expect(invokeMock.mock.calls[0][1]).not.toHaveProperty("message");
  });
});
