import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AvailableSkillUpdatesDialog } from "@/components/skills/AvailableSkillUpdatesDialog";
import type {
  LibrarySkill,
  LibrarySkillUpdateCheckResult,
  LibrarySkillUpdateResult,
} from "@/lib/api/skills";

const { mutateAsync } = vi.hoisted(() => ({ mutateAsync: vi.fn() }));
vi.mock("@/hooks/useSkills", () => ({
  useApplyLibrarySkillUpdate: () => ({ mutateAsync, isPending: false }),
}));
const key = "skills.library.update.";
function item(
  id: string,
  overrides: Partial<LibrarySkillUpdateCheckResult> = {},
) {
  const skill: LibrarySkill = {
    id,
    directory: id,
    displayName: id,
    source: { kind: "git" },
    compatibility: {
      claude: { compatible: true, issues: [] },
      codex: { compatible: true, issues: [] },
    },
    contentHash: "old",
    acquiredAt: 0,
    updatedAt: 0,
  };
  const check: LibrarySkillUpdateCheckResult = {
    librarySkillId: id,
    outcome: "update_available",
    observationToken: `observe-${id}`,
    stageToken: `stage-${id}`,
    recordedContentHash: "old",
    localModified: false,
    affectedDeployments: [],
    ...overrides,
  };
  return { skill, check };
}
function result(
  id: string,
  outcome: LibrarySkillUpdateResult["outcome"] = "updated",
): LibrarySkillUpdateResult {
  return {
    librarySkillId: id,
    outcome,
    affectedDeployments: [],
    backupPath: `/backups/${id}`,
  };
}
function setup(items = [item("A"), item("B")]) {
  const props = {
    open: true,
    items,
    onOpenChange: vi.fn(),
    onUpdated: vi.fn(),
    onRecheck: vi.fn().mockResolvedValue(undefined),
  };
  return { ...render(<AvailableSkillUpdatesDialog {...props} />), props };
}
const update = (id: string) =>
  screen.getByRole("button", { name: `${key}updateOne ${id}` });
const all = () => screen.getByRole("button", { name: `${key}updateAll` });
beforeEach(() => {
  mutateAsync.mockReset();
  mutateAsync.mockImplementation(async ({ librarySkillId }) =>
    result(librarySkillId),
  );
});

describe("AvailableSkillUpdatesDialog", () => {
  it("requires an explicit action and applies the exact reviewed snapshot", async () => {
    const { props, rerender } = setup();
    expect(mutateAsync).not.toHaveBeenCalled();
    fireEvent.click(update("A"));
    await waitFor(() => expect(props.onUpdated).toHaveBeenCalledWith("A"));
    expect(mutateAsync).toHaveBeenCalledWith({
      librarySkillId: "A",
      observationToken: "observe-A",
      stageToken: "stage-A",
      confirmLocalModifications: false,
    });
    rerender(<AvailableSkillUpdatesDialog {...props} items={[item("B")]} />);
    expect(screen.getByText("/backups/A")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `${key}updateOne A` }),
    ).not.toBeInTheDocument();
  });

  it("updates sequentially and blocks duplicate operations and dismissal while busy", async () => {
    let resolve!: (value: LibrarySkillUpdateResult) => void;
    mutateAsync.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const { props } = setup();
    fireEvent.click(all());
    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(all()).toBeDisabled();
    expect(screen.getByRole("button", { name: "common.close" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: `${key}recheck` }),
    ).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(props.onOpenChange).not.toHaveBeenCalled();
    await act(async () => resolve(result("A")));
    await waitFor(() => expect(props.onUpdated).toHaveBeenCalledTimes(2));
    expect(
      mutateAsync.mock.calls.map(([intent]) => intent.librarySkillId),
    ).toEqual(["A", "B"]);
  });

  it("binds local-edit consent to the reviewed snapshot", async () => {
    const { props, rerender } = setup([item("A", { localModified: true })]);
    expect(update("A")).toBeDisabled();
    expect(all()).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    expect(update("A")).toBeEnabled();
    rerender(
      <AvailableSkillUpdatesDialog
        {...props}
        items={[item("A", { localModified: true, stageToken: "new-stage" })]}
      />,
    );
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(update("A")).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(update("A"));
    await waitFor(() => expect(props.onUpdated).toHaveBeenCalledWith("A"));
    expect(mutateAsync.mock.calls[0][0]).toMatchObject({
      stageToken: "new-stage",
      confirmLocalModifications: true,
    });
  });

  it("excludes missing stages and incompatible deployments from batch updates", async () => {
    setup([
      item("A", { stageToken: undefined }),
      item("B", {
        affectedDeployments: [
          {
            currentCompatible: true,
            stagedCompatible: false,
            inspection: {} as never,
          },
        ],
      }),
      item("C"),
    ]);
    expect(update("A")).toBeDisabled();
    expect(update("B")).toBeDisabled();
    fireEvent.click(all());
    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync.mock.calls[0][0].librarySkillId).toBe("C");
  });

  it.each(["stale", "blocked", "rolled_back"] as const)(
    "retains %s results and requires a new check before retrying",
    async (outcome) => {
      mutateAsync.mockResolvedValue(result("A", outcome));
      const { props, rerender } = setup([item("A")]);
      fireEvent.click(update("A"));
      await screen.findByText(`${key}applyOutcome.${outcome}`);
      expect(props.onUpdated).not.toHaveBeenCalled();
      expect(update("A")).toBeDisabled();
      expect(screen.getByText("/backups/A")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: `${key}recheck` }));
      await waitFor(() => expect(props.onRecheck).toHaveBeenCalledOnce());
      rerender(
        <AvailableSkillUpdatesDialog
          {...props}
          items={[item("A", { stageToken: "new" })]}
        />,
      );
      await waitFor(() => expect(update("A")).toBeEnabled());
      expect(
        screen.queryByText(`${key}applyOutcome.${outcome}`),
      ).not.toBeInTheDocument();
    },
  );

  it("does not display old success as the result of a newly staged update", async () => {
    const { props, rerender } = setup([item("A")]);
    fireEvent.click(update("A"));
    await screen.findByText(`${key}applyOutcome.updated`);
    rerender(
      <AvailableSkillUpdatesDialog
        {...props}
        items={[item("A", { stageToken: "new" })]}
      />,
    );
    expect(update("A")).toBeEnabled();
    expect(
      screen.queryByText(`${key}applyOutcome.updated`),
    ).not.toBeInTheDocument();
  });

  it("stops the batch and preserves recovery evidence", async () => {
    mutateAsync.mockResolvedValue(result("A", "recovery_required"));
    const { props, rerender } = setup();
    fireEvent.click(all());
    await screen.findByText(`${key}batchStopped`);
    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(props.onUpdated).not.toHaveBeenCalled();
    rerender(
      <AvailableSkillUpdatesDialog
        {...props}
        items={[item("A", { stageToken: "new" }), item("B")]}
      />,
    );
    expect(update("A")).toBeDisabled();
    expect(
      screen.getByText(`${key}applyOutcome.recovery_required`),
    ).toBeInTheDocument();
  });

  it("retains transport errors and cannot reuse an uncertain stage", async () => {
    mutateAsync.mockRejectedValue(new Error("Connection lost"));
    const { props } = setup([item("A")]);
    fireEvent.click(update("A"));
    await screen.findByText("Connection lost");
    expect(update("A")).toBeDisabled();
    expect(props.onUpdated).not.toHaveBeenCalled();
  });
});
