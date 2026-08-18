import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsMigrationGate } from "@/components/skills/SkillsMigrationGate";
import type {
  SkillsMigrationExecutionResult,
  SkillsMigrationPreflight,
} from "@/lib/api/skills";

const { applyState, preflightState, restoreState, resumeState, revealState } =
  vi.hoisted(() => ({
    preflightState: {
      data: undefined as SkillsMigrationPreflight | undefined,
      isLoading: false,
      isFetching: false,
      isError: false,
      refetch: vi.fn(),
    },
    applyState: { isPending: false, mutateAsync: vi.fn() },
    resumeState: { isPending: false, mutateAsync: vi.fn() },
    restoreState: { isPending: false, mutateAsync: vi.fn() },
    revealState: { isPending: false, mutateAsync: vi.fn() },
  }));

vi.mock("@/hooks/useSkills", () => ({
  useSkillsMigrationPreflight: () => preflightState,
  useApplySkillsMigration: () => applyState,
  useResumeSkillsMigration: () => resumeState,
  useRestoreSkillsMigrationBackup: () => restoreState,
  useRevealSkillsMigrationPlanItem: () => revealState,
}));

const execution = (
  outcome: SkillsMigrationExecutionResult["outcome"],
  overrides: Partial<SkillsMigrationExecutionResult> = {},
): SkillsMigrationExecutionResult => ({
  outcome,
  pageMode: "read_only",
  progress: { completedItems: 1, totalItems: 3 },
  items: [],
  ...overrides,
});

describe("SkillsMigrationGate", () => {
  beforeEach(() => {
    preflightState.data = undefined;
    preflightState.isLoading = false;
    preflightState.isFetching = false;
    preflightState.isError = false;
    preflightState.refetch.mockReset().mockResolvedValue(undefined);
    applyState.isPending = false;
    applyState.mutateAsync.mockReset();
    resumeState.isPending = false;
    resumeState.mutateAsync.mockReset();
    restoreState.isPending = false;
    restoreState.mutateAsync.mockReset();
    revealState.isPending = false;
    revealState.mutateAsync.mockReset();
  });

  it("explains unsupported Consumers and requires explicit preservation consent", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "unsupported-consumer-plan",
      pageMode: "read_only",
      inventory: [],
      plan: [
        {
          disposition: "user_resolve",
          action: "preserve_unsupported_consumer_files",
          directory: "computer-use",
          fromLocation: "/legacy/skills/computer-use",
          reason: "unsupported_consumer_enabled",
          unsupportedConsumers: ["hermes"],
        },
      ],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: false,
        contentPaths: ["/legacy/skills/computer-use"],
      },
    };
    applyState.mutateAsync.mockResolvedValueOnce(execution("completed"));
    const user = userEvent.setup();
    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(
      screen.getByText(
        "skills.migration.guidance.unsupported_consumer_enabled",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.legacyConsumer.hermes"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.migration.apply" }),
    );
    const confirm = screen.getByRole("button", {
      name: "skills.migration.confirm.apply",
    });
    expect(confirm).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", {
        name: "skills.migration.confirm.preserveUnsupportedConsumers",
      }),
    );
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(applyState.mutateAsync).toHaveBeenCalledWith({
      observationToken: "unsupported-consumer-plan",
      preserveUnsupportedConsumerFiles: true,
    });
  });

  it("offers concrete guidance and Finder reveal for filesystem conflicts", async () => {
    preflightState.data = {
      status: "blocked",
      observationToken: "conflict-plan",
      pageMode: "read_only",
      inventory: [],
      plan: [
        {
          disposition: "user_resolve",
          action: "resolve_conflict",
          directory: "foreign",
          fromLocation: "/legacy/skills/foreign",
          reason: "foreign_or_ambiguous",
        },
      ],
      backup: {
        required: true,
        ready: false,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };
    revealState.mutateAsync.mockResolvedValueOnce(true);
    const user = userEvent.setup();
    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(
      screen.getByText("skills.migration.guidance.foreign_or_ambiguous"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.migration.revealInFinder" }),
    );
    expect(revealState.mutateAsync).toHaveBeenCalledWith({
      observationToken: "conflict-plan",
      planIndex: 0,
    });
  });

  it("renders the writable redesigned page only when migration needs no decision", () => {
    preflightState.data = {
      status: "not_required",
      observationToken: "no-migration",
      pageMode: "writable",
      inventory: [],
      plan: [],
      backup: {
        required: false,
        ready: true,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };

    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(
      screen.getByRole("button", { name: "mutable-skills-action" }),
    ).toBeEnabled();
    expect(
      screen.queryByText("skills.migration.title"),
    ).not.toBeInTheDocument();
  });

  it("keeps stale writable data read-only while a refetch is pending", () => {
    preflightState.data = {
      status: "not_required",
      observationToken: "no-migration",
      pageMode: "writable",
      inventory: [],
      plan: [],
      backup: {
        required: false,
        ready: true,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };
    const onReadOnlyChange = vi.fn();
    const view = render(
      <SkillsMigrationGate enabled onReadOnlyChange={onReadOnlyChange}>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    preflightState.isFetching = true;
    view.rerender(
      <SkillsMigrationGate enabled onReadOnlyChange={onReadOnlyChange}>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(screen.getByText("skills.migration.title")).toBeInTheDocument();
    expect(onReadOnlyChange).toHaveBeenLastCalledWith(true);
  });

  it("fails closed when a refetch errors after cached writable data", () => {
    preflightState.data = {
      status: "not_required",
      observationToken: "no-migration",
      pageMode: "writable",
      inventory: [],
      plan: [],
      backup: {
        required: false,
        ready: true,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };
    const onReadOnlyChange = vi.fn();
    const view = render(
      <SkillsMigrationGate enabled onReadOnlyChange={onReadOnlyChange}>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    preflightState.isError = true;
    view.rerender(
      <SkillsMigrationGate enabled onReadOnlyChange={onReadOnlyChange}>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(screen.getByText("skills.migration.loadError")).toBeInTheDocument();
    expect(onReadOnlyChange).toHaveBeenLastCalledWith(true);
  });

  it.each([
    ["loading", { isLoading: true }],
    ["error", { isError: true }],
    [
      "decision",
      {
        data: {
          status: "decision_needed",
          observationToken: "plan-v1",
          pageMode: "read_only",
          inventory: [],
          plan: [],
          backup: {
            required: true,
            ready: false,
            recoveryAvailable: false,
            contentPaths: [],
          },
        },
      },
    ],
    [
      "blocked",
      {
        data: {
          status: "blocked",
          observationToken: "plan-v1",
          pageMode: "read_only",
          inventory: [],
          plan: [],
          backup: {
            required: true,
            ready: false,
            recoveryAvailable: false,
            contentPaths: [],
          },
        },
      },
    ],
  ] as const)("reports the %s state as read-only to App", (_, state) => {
    Object.assign(preflightState, state);
    const onReadOnlyChange = vi.fn();

    render(
      <SkillsMigrationGate enabled onReadOnlyChange={onReadOnlyChange}>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(onReadOnlyChange).toHaveBeenLastCalledWith(true);
    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
  });

  it("does not offer Apply while backup prerequisites are unavailable", () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [
        {
          kind: "legacy_skill",
          directory: "review",
          consumer: "claude",
          location: "/legacy/skills/review",
          state: "real_directory",
          enabled: true,
        },
        {
          kind: "target_conflict",
          directory: "audit",
          consumer: "codex",
          location: "/target/skills/audit",
          state: "foreign_link",
        },
      ],
      plan: [
        {
          disposition: "perform",
          action: "move_to_library",
          directory: "review",
          fromLocation: "/legacy/skills/review",
          toLocation: "/library/review",
          reason: "proven_managed",
        },
        {
          disposition: "user_resolve",
          action: "resolve_conflict",
          directory: "audit",
          reason: "foreign_or_ambiguous",
        },
      ],
      backup: {
        required: true,
        ready: false,
        recoveryAvailable: true,
        databasePath: "/backup/cc-switch.db",
        contentPaths: ["/backup/skills/review"],
      },
    };

    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(screen.getAllByText("review")).toHaveLength(2);
    expect(
      screen.getByText("skills.library.consumerClaude"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.library.consumerCodex"),
    ).toBeInTheDocument();
    expect(
      within(screen.getAllByTestId("migration-inventory-item")[0]).queryByText(
        "claude",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getAllByTestId("migration-inventory-item")[1]).queryByText(
        "codex",
      ),
    ).not.toBeInTheDocument();
    expect(screen.getByText("/legacy/skills/review")).toBeInTheDocument();
    expect(screen.getByText("/target/skills/audit")).toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.disposition.perform"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.disposition.user_resolve"),
    ).toBeInTheDocument();
    expect(screen.getByText("/backup/cc-switch.db")).toBeInTheDocument();
    expect(screen.getByText("/backup/skills/review")).toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.backup.notReady"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /apply/i }),
    ).not.toBeInTheDocument();
  });

  it("requires explicit confirmation before applying the reviewed token", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };
    applyState.mutateAsync.mockResolvedValueOnce(execution("completed"));
    const user = userEvent.setup();
    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.migration.apply" }),
    );
    expect(applyState.mutateAsync).not.toHaveBeenCalled();
    expect(
      screen.getByText("skills.migration.confirm.description"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.migration.confirm.apply" }),
    );

    expect(applyState.mutateAsync).toHaveBeenCalledWith({
      observationToken: "migration-plan-v1",
      preserveUnsupportedConsumerFiles: false,
    });
    expect(preflightState.refetch).toHaveBeenCalled();
    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
  });

  it("refetches stale observations and requires the replacement plan to be reviewed", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: false,
        contentPaths: [],
      },
    };
    applyState.mutateAsync.mockResolvedValueOnce(
      execution("stale_observation"),
    );
    const user = userEvent.setup();
    render(<SkillsMigrationGate enabled />);

    await user.click(
      screen.getByRole("button", { name: "skills.migration.apply" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.migration.confirm.apply" }),
    );

    expect(preflightState.refetch).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert")).toContainElement(
      screen.getByText("skills.migration.execution.stale_observation"),
    );
    expect(
      screen.queryByRole("button", { name: "skills.migration.confirm.apply" }),
    ).not.toBeInTheDocument();
  });

  it("shows persisted resumable progress with only Resume and Recheck actions", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("resumable"),
    };
    resumeState.mutateAsync.mockResolvedValueOnce(execution("resumable"));
    const user = userEvent.setup();
    render(<SkillsMigrationGate enabled />);

    expect(screen.getByText("1 / 3")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.migration.apply" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.migration.defer" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.migration.resume" }),
    );
    expect(resumeState.mutateAsync).toHaveBeenCalledWith();
  });

  it("also offers Restore for a resumable run with a verified backup", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("resumable", {
        backup: {
          backupId: "opaque-backup-v1",
          createdAt: 1_700_000_000_000,
          restoreAvailable: true,
        },
      }),
    };
    restoreState.mutateAsync.mockResolvedValueOnce(execution("restored"));
    const user = userEvent.setup();
    render(<SkillsMigrationGate enabled />);

    expect(
      screen.getByRole("button", { name: "skills.migration.resume" }),
    ).toBeEnabled();
    await user.click(
      screen.getByRole("button", { name: "skills.migration.restore" }),
    );

    expect(restoreState.mutateAsync).toHaveBeenCalledWith("opaque-backup-v1");
  });

  it("offers Resume and Restore for a blocked partial run with a verified backup", async () => {
    preflightState.data = {
      status: "blocked",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("blocked", {
        backup: {
          backupId: "opaque-backup-v1",
          createdAt: 1_700_000_000_000,
          restoreAvailable: true,
        },
      }),
    };
    resumeState.mutateAsync.mockResolvedValueOnce(execution("resumable"));
    restoreState.mutateAsync.mockResolvedValueOnce(execution("restored"));
    const user = userEvent.setup();
    render(<SkillsMigrationGate enabled />);

    expect(
      screen.queryByRole("button", { name: "skills.migration.apply" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.migration.defer" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.migration.resume" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.migration.restore" }),
    );

    expect(resumeState.mutateAsync).toHaveBeenCalledWith();
    expect(restoreState.mutateAsync).toHaveBeenCalledWith("opaque-backup-v1");
  });

  it("explains a blocked pre-backup run and leaves only Recheck available", () => {
    preflightState.data = {
      status: "blocked",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: false,
        recoveryAvailable: false,
        contentPaths: [],
      },
      execution: execution("blocked", {
        progress: { completedItems: 0, totalItems: 3 },
      }),
    };
    render(<SkillsMigrationGate enabled />);

    expect(
      screen.getByText("skills.migration.execution.blockedNoBackup"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.migration.recheck" }),
    ).toBeEnabled();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("offers opaque backup restore for high-visibility recovery", async () => {
    preflightState.data = {
      status: "blocked",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: false,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("recovery_required", {
        backup: {
          backupId: "opaque-backup-v1",
          createdAt: 1_700_000_000_000,
          restoreAvailable: true,
        },
      }),
    };
    restoreState.mutateAsync.mockResolvedValueOnce(execution("restored"));
    const user = userEvent.setup();
    render(<SkillsMigrationGate enabled />);

    expect(
      screen.getByText("skills.migration.execution.recovery_required"),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toContainElement(
      screen.getByText("skills.migration.execution.recovery_required"),
    );
    expect(
      screen.queryByRole("button", { name: "skills.migration.resume" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "skills.migration.restore" }),
    );
    expect(restoreState.mutateAsync).toHaveBeenCalledWith("opaque-backup-v1");
  });

  it("keeps a completed migration closed while authoritative preview refreshes", () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("completed", {
        pageMode: "writable",
        progress: { completedItems: 3, totalItems: 3 },
      }),
    };

    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.execution.awaitingPreview"),
    ).toBeInTheDocument();
  });

  it("fails closed when a nominally writable preview still reports persisted execution", () => {
    preflightState.data = {
      status: "not_required",
      observationToken: "post-migration",
      pageMode: "writable",
      inventory: [],
      plan: [],
      backup: {
        required: false,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
      execution: execution("completed", {
        pageMode: "writable",
        progress: { completedItems: 3, totalItems: 3 },
      }),
    };

    render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(
      screen.getByText("skills.migration.execution.awaitingPreview"),
    ).toBeInTheDocument();
  });

  it("defers only in session and keeps the redesigned page read-only", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [],
      plan: [],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
    };
    const user = userEvent.setup();

    function SessionGate() {
      const [deferredToken, setDeferredToken] = useState<string | null>(null);
      return (
        <SkillsMigrationGate
          deferredToken={deferredToken}
          enabled
          onDefer={setDeferredToken}
        >
          <button type="button">mutable-skills-action</button>
        </SkillsMigrationGate>
      );
    }
    render(<SessionGate />);
    await user.click(
      screen.getByRole("button", { name: "skills.migration.defer" }),
    );

    expect(
      screen.getByText("skills.migration.deferredDescription"),
    ).toBeInTheDocument();
    expect(screen.queryByText("mutable-skills-action")).not.toBeInTheDocument();
    expect(preflightState.refetch).not.toHaveBeenCalled();
  });

  it("preserves the reviewed plan during recheck and reports token drift", async () => {
    preflightState.data = {
      status: "decision_needed",
      observationToken: "migration-plan-v1",
      pageMode: "read_only",
      inventory: [
        {
          kind: "legacy_skill",
          directory: "review",
          location: "/legacy/review",
          state: "present",
        },
      ],
      plan: [
        {
          disposition: "perform",
          action: "move_to_library",
          directory: "review",
          reason: "proven_managed",
        },
      ],
      backup: {
        required: true,
        ready: true,
        recoveryAvailable: true,
        contentPaths: [],
      },
    };
    let resolveRecheck:
      | ((value: { data: SkillsMigrationPreflight }) => void)
      | undefined;
    preflightState.refetch.mockImplementationOnce(
      () =>
        new Promise<{ data: SkillsMigrationPreflight }>((resolve) => {
          resolveRecheck = resolve;
        }),
    );
    const user = userEvent.setup();
    const view = render(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.migration.recheck" }),
    );
    preflightState.isFetching = true;
    view.rerender(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );
    expect(screen.getByText("/legacy/review")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.migration.recheck" }),
    ).toBeDisabled();

    const changed: SkillsMigrationPreflight = {
      ...preflightState.data,
      observationToken: "migration-plan-v2",
      inventory: [
        ...preflightState.data.inventory,
        {
          kind: "unmanaged_content",
          directory: "new-drift",
          location: "/legacy/new-drift",
          state: "real_directory",
        },
      ],
    };
    resolveRecheck?.({ data: changed });
    preflightState.data = changed;
    preflightState.isFetching = false;
    view.rerender(
      <SkillsMigrationGate enabled>
        <button type="button">mutable-skills-action</button>
      </SkillsMigrationGate>,
    );

    expect(
      await screen.findByText("skills.migration.drift.changed"),
    ).toHaveAttribute("role", "alert");
    expect(screen.getByText("/legacy/new-drift")).toBeInTheDocument();
  });
});
