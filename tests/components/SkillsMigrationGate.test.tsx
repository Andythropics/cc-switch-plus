import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsMigrationGate } from "@/components/skills/SkillsMigrationGate";
import type { SkillsMigrationPreflight } from "@/lib/api/skills";

const { preflightState } = vi.hoisted(() => ({
  preflightState: {
    data: undefined as SkillsMigrationPreflight | undefined,
    isLoading: false,
    isFetching: false,
    isError: false,
    refetch: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useSkillsMigrationPreflight: () => preflightState,
}));

describe("SkillsMigrationGate", () => {
  beforeEach(() => {
    preflightState.data = undefined;
    preflightState.isLoading = false;
    preflightState.isFetching = false;
    preflightState.isError = false;
    preflightState.refetch.mockReset().mockResolvedValue(undefined);
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

  it("shows the deterministic inventory, plan, and recovery prerequisites without an apply action", () => {
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
