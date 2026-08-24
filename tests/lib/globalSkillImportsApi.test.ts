import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { globalSkillImportsApi } from "@/lib/api/globalSkillImports";

describe("globalSkillImportsApi", () => {
  beforeEach(() => invokeMock.mockReset());

  it("inspects without a caller-supplied root", async () => {
    invokeMock.mockResolvedValue({ observationToken: "token", findings: [] });

    await globalSkillImportsApi.inspect();

    expect(invokeMock).toHaveBeenCalledWith("inspectGlobalSkillImports");
  });

  it("applies only finding identity, token, mode, and resolution", async () => {
    const intent = {
      findingId: "global:finding",
      observationToken: "token",
      mode: "import_and_replace" as const,
      resolution: {
        kind: "create_new" as const,
        directory: "managed-skill",
      },
    };
    invokeMock.mockResolvedValue({
      findingId: intent.findingId,
      outcome: "deployed",
    });

    await globalSkillImportsApi.apply(intent);

    expect(invokeMock).toHaveBeenCalledWith("applyGlobalSkillImport", {
      intent,
    });
    expect(JSON.stringify(invokeMock.mock.calls[0])).not.toContain(
      "sourcePath",
    );
  });
});
