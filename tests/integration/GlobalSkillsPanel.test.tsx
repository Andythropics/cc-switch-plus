import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { GlobalSkillsPanel } from "@/components/skills/GlobalSkillsPanel";
import type { GlobalSkillsPanelHandle } from "@/components/skills/GlobalSkillsPanel";
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

const globalDeployment = (
  skill: LibrarySkill,
  consumer: "claude" | "codex",
) => ({
  librarySkillId: skill.id,
  libraryDirectory: skill.directory,
  target: { consumer, workspace: "global" },
  desired: {
    id: `desired-${skill.id}-${consumer}`,
    librarySkillId: skill.id,
    libraryDirectory: skill.directory,
    target: { consumer, workspace: "global" },
    createdAt: 1,
    updatedAt: 1,
  },
  observed: {
    state: "correct_link",
    targetPath: `/global/${skill.directory}`,
    expectedTarget: `/library/${skill.directory}`,
  },
  observationToken: `${skill.id}-${consumer}-token`,
  status: "in_sync",
});

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
  const panelRef = createRef<GlobalSkillsPanelHandle>();
  const view = render(
    <QueryClientProvider client={client}>
      <GlobalSkillsPanel ref={panelRef} />
    </QueryClientProvider>,
  );
  return { ...view, panelRef };
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
          return {
            items: skills.map((skill) =>
              globalDeployment(
                skill,
                (args as { query?: { consumer?: "claude" | "codex" } })?.query
                  ?.consumer ?? "claude",
              ),
            ),
          };
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

  it("loads real queries, submits ordered Global undeploy batch, and renders partial outcomes", async () => {
    const user = userEvent.setup();
    const { panelRef } = renderGlobal();

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

    await act(async () => {
      panelRef.current?.openBatchUndeploy();
    });
    await screen.findByTestId("batch-skill-skill-a");
    const actionSelect = screen.getByRole("combobox", {
      name: "skills.batch.action",
    });
    expect(actionSelect).toHaveTextContent("skills.batch.undeploy");
    expect(actionSelect).toBeDisabled();
    expect(
      screen.getByRole("combobox", { name: "skills.batch.target" }),
    ).toBeDisabled();
    await user.click(screen.getByTestId("batch-apply"));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("applySkillDeployments", {
        batch: {
          intents: [
            {
              action: "undeploy",
              librarySkillId: "skill-a",
              target: { consumer: "claude", workspace: "global" },
            },
            {
              action: "undeploy",
              librarySkillId: "skill-a",
              target: { consumer: "codex", workspace: "global" },
            },
            {
              action: "undeploy",
              librarySkillId: "skill-b",
              target: { consumer: "claude", workspace: "global" },
            },
            {
              action: "undeploy",
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

  it("leaves global recovery inspection to the shared Skills navigation", async () => {
    renderGlobal();

    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("inspectDeploymentRecovery", {
      query: { workspace: "global" },
    });
    expect(screen.queryByText("skills.recovery.title")).not.toBeInTheDocument();
  });
});
