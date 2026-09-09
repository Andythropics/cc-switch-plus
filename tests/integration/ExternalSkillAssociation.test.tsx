import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ExternalSkillUpdatesPanel } from "@/components/skills/ExternalSkillUpdatesPanel";
import { SkillsMigrationGate } from "@/components/skills/SkillsMigrationGate";
import type { LibrarySkill } from "@/lib/api/skills";

const state = vi.hoisted(() => ({
  apply: vi.fn(),
  link: vi.fn(),
  preflight: vi.fn(),
  scan: vi.fn(),
}));
const skill: LibrarySkill = {
  id: "library",
  directory: "implementation",
  displayName: "Implementation",
  source: { kind: "local_import" },
  contentHash: "old",
  acquiredAt: 1,
  updatedAt: 1,
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
};
vi.mock("@/lib/api/externalSkillUpdates", () => ({
  externalSkillUpdatesApi: {
    inspect: async () => {
      state.scan();
      return {
        observationToken: "observed",
        warnings: [],
        candidates: [
          {
            targetObservationTokens: { library: "scoped-implementation" },
            id: "implementation",
            directory: "implementation",
            source: {
              kind: "git",
              repoOwner: "owner",
              repoName: "repo",
              skillPath: "skills/implementation",
            },
            contentHash: "new",
            librarySkillId: null,
            suggestedLibrarySkillIds: ["library"],
            changed: true,
            localModified: false,
            deploymentReplaced: false,
          },
        ].flatMap((candidate) => [
          candidate,
          {
            ...candidate,
            id: "planning",
            directory: "planning",
            suggestedLibrarySkillIds: ["library2"],
            targetObservationTokens: { library2: "scoped-planning" },
          },
        ]),
      };
    },
    apply: state.apply,
    link: state.link,
    linkSource: vi.fn(),
  },
}));
vi.mock("@/hooks/useSkills", async () => {
  const { useQuery } = await import("@tanstack/react-query");
  const writable = {
    status: "not_required",
    pageMode: "writable",
    observationToken: "migration",
    inventory: [],
    plan: [],
    backup: {
      required: false,
      ready: true,
      recoveryAvailable: false,
      contentPaths: [],
    },
  };
  const idle = () => ({ mutateAsync: vi.fn(), isPending: false });
  return {
    useSkillsMigrationPreflight: () =>
      useQuery({
        queryKey: ["skills", "migrationPreflight"],
        initialData: writable,
        staleTime: Infinity,
        queryFn: async () => {
          state.preflight();
          await new Promise((resolve) => setTimeout(resolve, 10));
          return writable;
        },
      }),
    useSkillsMigrationReport: () => ({ data: null }),
    useApplySkillsMigration: idle,
    useAcknowledgeSkillsMigrationReport: idle,
    useResumeSkillsMigration: idle,
    useRestoreSkillsMigrationBackup: idle,
    useRevealSkillsMigrationFinding: idle,
    useRevealSkillsMigrationPlanItem: idle,
  };
});

describe("CLI association inside the migration gate", () => {
  it.each(["skills.external.linkAndUpdate", "skills.external.linkOnly"])(
    "returns to the installation list after %s",
    async (action) => {
      state.apply.mockResolvedValue({ outcome: "updated" });
      state.link.mockResolvedValue(skill);
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      render(
        <QueryClientProvider client={client}>
          <SkillsMigrationGate enabled>
            <ExternalSkillUpdatesPanel
              skills={[
                skill,
                { ...skill, id: "library2", directory: "planning" },
              ]}
            />
          </SkillsMigrationGate>
        </QueryClientProvider>,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "skills.external.syncCli" }),
      );
      fireEvent.click(
        (
          await screen.findAllByRole("button", {
            name: "skills.external.select",
          })
        )[0],
      );
      fireEvent.click(screen.getByRole("button", { name: action }));
      await waitFor(() =>
        expect(
          action.endsWith("linkOnly") ? state.link : state.apply,
        ).toHaveBeenCalled(),
      );
      expect(
        action.endsWith("linkOnly") ? state.link : state.apply,
      ).toHaveBeenCalledWith(
        expect.objectContaining({
          restoreDeployment: !action.endsWith("linkOnly"),
          observationToken: "scoped-implementation",
        }),
        expect.anything(),
      );
      await waitFor(() => {
        const dialog = screen.getByRole("dialog");
        expect(
          within(dialog).getByRole("button", {
            name: "skills.external.select",
          }),
        ).toBeInTheDocument();
        expect(
          within(dialog).queryByLabelText("skills.external.target"),
        ).not.toBeInTheDocument();
      });
      expect(state.scan).toHaveBeenCalledTimes(1);
      expect(
        within(screen.getByRole("dialog")).queryByText("implementation"),
      ).not.toBeInTheDocument();
      expect(
        within(screen.getByRole("dialog")).getByText("planning"),
      ).toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: "skills.external.select" }),
      );
      fireEvent.click(screen.getByRole("button", { name: action }));
      await waitFor(() =>
        expect(
          screen.getByText("skills.external.allSynced"),
        ).toBeInTheDocument(),
      );
      expect(
        action.endsWith("linkOnly") ? state.link : state.apply,
      ).toHaveBeenLastCalledWith(
        expect.objectContaining({
          candidateId: "planning",
          observationToken: "scoped-planning",
        }),
        expect.anything(),
      );
      expect(state.scan).toHaveBeenCalledTimes(1);
      fireEvent.click(
        screen.getByRole("button", { name: "skills.external.rescan" }),
      );
      await waitFor(() =>
        expect(
          screen.getAllByRole("button", { name: "skills.external.select" }),
        ).toHaveLength(2),
      );
      expect(state.scan).toHaveBeenCalledTimes(2);
      expect(state.preflight).not.toHaveBeenCalled();
    },
  );
});
