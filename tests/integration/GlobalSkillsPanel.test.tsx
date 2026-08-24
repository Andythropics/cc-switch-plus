import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { GlobalSkillsPanel } from "@/components/skills/GlobalSkillsPanel";
import type { LibrarySkill } from "@/lib/api/skills";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

// Keep the real hooks and API modules in this test. Only the Tauri boundary is
// mocked so query keys, invalidation, payload shape, and rendering stay real.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const skills: LibrarySkill[] = [
  {
    id: "skill-a",
    directory: "alpha",
    displayName: "Alpha",
    description: "First skill",
    source: { kind: "local_import" },
    compatibility: {
      claude: { compatible: true, issues: [] },
      codex: { compatible: true, issues: [] },
    },
    contentHash: "hash-a",
    acquiredAt: 1,
    updatedAt: 1,
  },
  {
    id: "skill-b",
    directory: "beta",
    displayName: "Beta",
    description: "Second skill",
    source: { kind: "local_import" },
    compatibility: {
      claude: { compatible: true, issues: [] },
      codex: { compatible: true, issues: [] },
    },
    contentHash: "hash-b",
    acquiredAt: 1,
    updatedAt: 1,
  },
];

const partialResult = {
  items: [
    {
      librarySkillId: "skill-a",
      target: { consumer: "claude", workspace: "global" },
      outcome: "applied",
      message: "linked Alpha to Claude",
    },
    {
      librarySkillId: "skill-a",
      target: { consumer: "codex", workspace: "global" },
      outcome: "blocked",
      message: "Codex target is unavailable",
    },
    {
      librarySkillId: "skill-b",
      target: { consumer: "claude", workspace: "global" },
      outcome: "applied",
    },
    {
      librarySkillId: "skill-b",
      target: { consumer: "codex", workspace: "global" },
      outcome: "applied",
    },
  ],
};

const renderGlobal = () => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={client}>
      <GlobalSkillsPanel />
    </QueryClientProvider>,
  );
};

describe("GlobalSkillsPanel Tauri-boundary integration", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string, args?: unknown) => {
      switch (command) {
        case "getLibrarySkills":
          return skills;
        case "listProjectWorkspaces":
          return [];
        case "inspectSkillDeployments":
          return { items: [] };
        case "inspectDeploymentRecovery":
          return {
            findings: [
              {
                disposition: "recoverable",
                target: { consumer: "claude", workspace: "global" },
                entryName: "alpha",
                librarySkillId: "skill-a",
                libraryDirectory: "alpha",
                observedTarget: "/private/library/alpha",
                observationToken: "recovery-token",
                safeReason: "exact_library_link",
              },
            ],
          };
        case "inspectGlobalSkillImports":
          return { observationToken: "global-empty", findings: [] };
        case "applySkillDeployments": {
          const batch = (
            args as { batch?: { intents?: { action?: string }[] } }
          )?.batch;
          if (batch?.intents?.[0]?.action === "recover") {
            return {
              items: [
                {
                  librarySkillId: "skill-a",
                  target: { consumer: "claude", workspace: "global" },
                  outcome: "applied",
                },
              ],
            };
          }
          return partialResult;
        }
        default:
          throw new Error(`Unexpected Tauri command: ${command}`);
      }
    });
  });

  it("loads real queries, submits ordered Global batch, and renders partial outcomes", async () => {
    const user = userEvent.setup();
    renderGlobal();

    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(await screen.findByText("Beta")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("getLibrarySkills");
    expect(invokeMock).toHaveBeenCalledWith("listProjectWorkspaces", {
      includeArchived: true,
    });
    expect(invokeMock).toHaveBeenCalledWith("inspectSkillDeployments", {
      query: { consumer: "claude", workspace: "global" },
    });
    expect(invokeMock).toHaveBeenCalledWith("inspectSkillDeployments", {
      query: { consumer: "codex", workspace: "global" },
    });

    await user.click(
      screen.getByRole("button", { name: "skills.global.batchDeploy" }),
    );
    await screen.findByTestId("batch-skill-skill-a");
    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("applySkillDeployments", {
        batch: {
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
        },
      }),
    );

    expect(await screen.findByTestId("batch-results")).toBeInTheDocument();
    expect(screen.getByTestId("batch-result-0")).toHaveTextContent("applied");
    expect(screen.getByTestId("batch-result-1")).toHaveTextContent("blocked");
    expect(screen.getByText("Codex target is unavailable")).toBeInTheDocument();
    expect(screen.getAllByTestId(/^batch-result-/)).toHaveLength(4);
  });

  it("inspects and explicitly confirms recovery through the real typed boundary", async () => {
    const user = userEvent.setup();
    renderGlobal();

    const checkbox = await screen.findByRole("checkbox", {
      name: "skills.recovery.select",
    });
    expect(invokeMock).toHaveBeenCalledWith("inspectDeploymentRecovery", {
      query: { workspace: "global" },
    });
    await user.click(checkbox);
    await user.click(
      screen.getByRole("button", {
        name: "skills.recovery.reviewSelected",
      }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "applySkillDeployments",
      expect.anything(),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.recovery.confirm" }),
    );

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("applySkillDeployments", {
        batch: {
          intents: [
            {
              action: "recover",
              librarySkillId: "skill-a",
              target: { consumer: "claude", workspace: "global" },
              observationToken: "recovery-token",
              confirmed: true,
            },
          ],
        },
      }),
    );
    expect(await screen.findByTestId("recovery-result-0")).toHaveTextContent(
      "skills.batch.outcome.applied",
    );
  });
});
