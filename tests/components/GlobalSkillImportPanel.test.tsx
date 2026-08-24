import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { GlobalSkillImportPanel } from "@/components/skills/GlobalSkillImportPanel";
import type { GlobalSkillImportInspection } from "@/lib/api/globalSkillImports";

const { applyMock } = vi.hoisted(() => ({ applyMock: vi.fn() }));

vi.mock("@/hooks/useSkills", () => ({
  useApplyGlobalSkillImport: () => ({
    mutateAsync: applyMock,
    isPending: false,
  }),
}));

const inspection: GlobalSkillImportInspection = {
  observationToken: "global-token",
  findings: [
    {
      id: "global:finding",
      consumer: "codex",
      sourcePath: "/home/test/.agents/skills/npx-skill",
      directory: "npx-skill",
      validation: { status: "valid", issues: [] },
      compatibility: {
        claude: { compatible: true, issues: [] },
        codex: { compatible: true, issues: [] },
      },
      libraryMatch: { kind: "none" },
      directoryCollision: {
        kind: "none",
        requested: "npx-skill",
        suggestions: [],
      },
      replaceEligibility: { eligible: true },
    },
  ],
};

describe("GlobalSkillImportPanel", () => {
  beforeEach(() => {
    applyMock.mockReset().mockResolvedValue({
      findingId: "global:finding",
      outcome: "deployed",
    });
  });

  it("requires confirmation, warns about npx, and sends no source path", async () => {
    const user = userEvent.setup();
    render(
      <GlobalSkillImportPanel
        inspection={inspection}
        isLoading={false}
        isFetching={false}
        onRefetch={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("checkbox"));
    await user.click(
      screen.getByRole("button", {
        name: "skills.global.import.modeImportAndReplace",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.global.import.submit" }),
    );

    expect(
      screen.getByText("skills.global.import.installerWarning"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "skills.global.import.replaceConfirm",
      }),
    );

    await waitFor(() => expect(applyMock).toHaveBeenCalledTimes(1));
    expect(applyMock).toHaveBeenCalledWith({
      findingId: "global:finding",
      observationToken: "global-token",
      mode: "import_and_replace",
      resolution: { kind: "create_new", directory: "npx-skill" },
    });
    expect(applyMock.mock.calls[0][0]).not.toHaveProperty("sourcePath");
  });
});
