import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  useLibrarySkills,
  useProjectWorkspaces,
  useSkillActivity,
} from "@/hooks/useSkills";
import type {
  DeploymentConsumer,
  LibrarySkill,
  SkillActivityEntry,
  SkillActivityOperation,
  SkillActivityOutcome,
  SkillActivityQuery,
  SkillActivityReason,
} from "@/lib/api/skills";

interface SkillsActivityPanelProps {
  onOpenLibrary?: (librarySkillId: string) => void;
  onOpenProjects?: (workspaceId: string) => void;
  onOpenGlobal?: () => void;
}

const OPERATIONS: SkillActivityOperation[] = [
  "library",
  "workspace",
  "deployment",
  "repair",
  "migration",
  "forget",
  "removal",
];

const OUTCOMES: SkillActivityOutcome[] = [
  "success",
  "no_op",
  "blocked",
  "conflict",
  "failed",
  "compensation_failed",
  "rolled_back",
];

const REASONS: SkillActivityReason[] = [
  "acquire",
  "import",
  "metadata_update",
  "update",
  "register",
  "rename",
  "archive",
  "restore",
  "relocate",
  "lifecycle_refresh",
  "deploy",
  "replace_foreign_link",
  "undeploy",
  "repair",
  "migrate",
  "migrate_item",
  "resume",
  "deployment_forget",
  "workspace_forget",
  "library_remove",
  "deployment_remove",
  "legacy_link_remove",
  "compensation_restore",
  "import_and_replace",
  "recover_deployment",
];

const CONSUMERS: DeploymentConsumer[] = ["claude", "codex"];

type ActivityFilters = Omit<SkillActivityQuery, "cursor">;

interface ActivityGroup {
  key: string;
  batchId?: string;
  itemCount?: number;
  entries: SkillActivityEntry[];
}

function groupEntries(entries: SkillActivityEntry[]): ActivityGroup[] {
  const groups = new Map<string, ActivityGroup>();
  for (const entry of entries) {
    const key = entry.batch?.batchId ?? `entry-${entry.id}`;
    const current = groups.get(key);
    if (current) {
      current.entries.push(entry);
      continue;
    }
    groups.set(key, {
      key,
      batchId: entry.batch?.batchId,
      itemCount: entry.batch?.itemCount,
      entries: [entry],
    });
  }

  return Array.from(groups.values()).map((group) => ({
    ...group,
    entries: group.batchId
      ? [...group.entries].sort(
          (left, right) =>
            (left.batch?.itemIndex ?? 0) - (right.batch?.itemIndex ?? 0),
        )
      : group.entries,
  }));
}

function displayNameById(skills: LibrarySkill[]) {
  return new Map(skills.map((skill) => [skill.id, skill.displayName]));
}

export function SkillsActivityPanel({
  onOpenLibrary,
  onOpenProjects,
  onOpenGlobal,
}: SkillsActivityPanelProps) {
  const { t } = useTranslation();
  const [operation, setOperation] = useState<SkillActivityOperation | "all">(
    "all",
  );
  const [reason, setReason] = useState<SkillActivityReason | "all">("all");
  const [outcome, setOutcome] = useState<SkillActivityOutcome | "all">("all");
  const [consumer, setConsumer] = useState<DeploymentConsumer | "all">("all");

  const query = useMemo<ActivityFilters>(() => {
    const next: ActivityFilters = { limit: 50 };
    if (operation !== "all") next.operation = operation;
    if (reason !== "all") next.reason = reason;
    if (outcome !== "all") next.outcome = outcome;
    if (consumer !== "all") next.consumer = consumer;
    return next;
  }, [consumer, operation, outcome, reason]);

  const activity = useSkillActivity(query);
  const { data: librarySkills = [] } = useLibrarySkills();
  const { data: projects = [] } = useProjectWorkspaces();
  const libraryNames = useMemo(
    () => displayNameById(librarySkills),
    [librarySkills],
  );
  const projectNames = useMemo(
    () => new Map(projects.map((project) => [project.id, project.displayName])),
    [projects],
  );
  const groups = useMemo(
    () => groupEntries(activity.entries),
    [activity.entries],
  );

  const renderTarget = (entry: SkillActivityEntry) => {
    const target = entry.target;
    if (!target) return null;

    return (
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {target.librarySkillId && (
          <>
            {onOpenLibrary && libraryNames.has(target.librarySkillId) ? (
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0 text-xs"
                onClick={() => onOpenLibrary(target.librarySkillId!)}
              >
                {libraryNames.get(target.librarySkillId) ??
                  target.librarySkillId}
              </Button>
            ) : (
              <span>
                {libraryNames.get(target.librarySkillId) ??
                  target.librarySkillId}
              </span>
            )}
          </>
        )}
        {target.workspaceKind === "global" && (
          <>
            {onOpenGlobal ? (
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0 text-xs"
                onClick={onOpenGlobal}
              >
                {t("skills.activity.openGlobal")}
              </Button>
            ) : (
              <span>{t("skills.activity.global")}</span>
            )}
          </>
        )}
        {target.workspaceKind === "project" && target.workspaceId && (
          <>
            {onOpenProjects && projectNames.has(target.workspaceId) ? (
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0 text-xs"
                onClick={() => onOpenProjects!(target.workspaceId!)}
              >
                {projectNames.get(target.workspaceId) ?? target.workspaceId}
              </Button>
            ) : (
              <span>
                {projectNames.get(target.workspaceId) ?? target.workspaceId}
              </span>
            )}
          </>
        )}
        {target.consumer && (
          <span>{t(`skills.activity.consumer.${target.consumer}`)}</span>
        )}
      </div>
    );
  };

  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-hidden px-5 py-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">
            {t("skills.activity.title")}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t("skills.activity.description")}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={t("skills.activity.refresh")}
          disabled={activity.isFetching}
          onClick={() => void activity.refetch()}
        >
          <RefreshCw
            className={`mr-1.5 h-4 w-4${activity.isFetching ? " animate-spin" : ""}`}
          />
          {t("skills.activity.refresh")}
        </Button>
      </div>

      <div className="mt-4 grid gap-2 sm:grid-cols-4">
        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t("skills.activity.operationFilter")}
          <select
            aria-label={t("skills.activity.operationFilter")}
            value={operation}
            onChange={(event) =>
              setOperation(event.target.value as SkillActivityOperation | "all")
            }
            className="h-9 rounded-md border bg-background px-2 text-sm text-foreground"
          >
            <option value="all">{t("skills.activity.all")}</option>
            {OPERATIONS.map((value) => (
              <option key={value} value={value}>
                {t(`skills.activity.operation.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t("skills.activity.reasonFilter")}
          <select
            aria-label={t("skills.activity.reasonFilter")}
            value={reason}
            onChange={(event) =>
              setReason(event.target.value as SkillActivityReason | "all")
            }
            className="h-9 rounded-md border bg-background px-2 text-sm text-foreground"
          >
            <option value="all">{t("skills.activity.all")}</option>
            {REASONS.map((value) => (
              <option key={value} value={value}>
                {t(`skills.activity.reason.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t("skills.activity.outcomeFilter")}
          <select
            aria-label={t("skills.activity.outcomeFilter")}
            value={outcome}
            onChange={(event) =>
              setOutcome(event.target.value as SkillActivityOutcome | "all")
            }
            className="h-9 rounded-md border bg-background px-2 text-sm text-foreground"
          >
            <option value="all">{t("skills.activity.all")}</option>
            {OUTCOMES.map((value) => (
              <option key={value} value={value}>
                {t(`skills.activity.outcome.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t("skills.activity.consumerFilter")}
          <select
            aria-label={t("skills.activity.consumerFilter")}
            value={consumer}
            onChange={(event) =>
              setConsumer(event.target.value as DeploymentConsumer | "all")
            }
            className="h-9 rounded-md border bg-background px-2 text-sm text-foreground"
          >
            <option value="all">{t("skills.activity.all")}</option>
            {CONSUMERS.map((value) => (
              <option key={value} value={value}>
                {t(`skills.activity.consumer.${value}`)}
              </option>
            ))}
          </select>
        </label>
      </div>

      {(activity.isPending || activity.isFetching) &&
        !activity.isFetchingNextPage && (
          <p role="status" className="mt-3 text-xs text-muted-foreground">
            {t("skills.activity.loading")}
          </p>
        )}
      {activity.isError && (
        <p
          role="alert"
          className="mt-3 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        >
          {t("skills.activity.loadError")}
        </p>
      )}

      <div className="mt-4 min-h-0 flex-1 space-y-3 overflow-y-auto">
        {!activity.isPending && !activity.isError && groups.length === 0 && (
          <p className="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
            {t("skills.activity.empty")}
          </p>
        )}
        {groups.map((group) => (
          <section
            key={group.key}
            className="rounded-lg border bg-card/50 p-3"
            data-testid={
              group.batchId
                ? `skills-activity-batch-${group.batchId}`
                : `skills-activity-single-${group.entries[0]?.id}`
            }
          >
            {group.batchId && (
              <div className="mb-2 flex items-center gap-2 text-sm font-medium">
                <span>{t("skills.activity.batch")}</span>
                <code>{group.batchId}</code>
                {group.itemCount && (
                  <span className="text-xs text-muted-foreground">
                    ({group.itemCount})
                  </span>
                )}
              </div>
            )}
            <div className="space-y-2">
              {group.entries.map((entry) => (
                <article
                  key={entry.id}
                  className="rounded-md border bg-background/50 p-3"
                  data-testid="skills-activity-entry"
                >
                  <div className="flex flex-wrap items-center gap-2 text-sm">
                    <time dateTime={new Date(entry.occurredAt).toISOString()}>
                      {new Date(entry.occurredAt).toLocaleString()}
                    </time>
                    <span className="font-medium">
                      {t(`skills.activity.operation.${entry.operation}`)}
                    </span>
                    <span>{t(`skills.activity.outcome.${entry.outcome}`)}</span>
                    {entry.batch && (
                      <span className="text-xs text-muted-foreground">
                        {entry.batch.itemIndex + 1}/{entry.batch.itemCount}
                      </span>
                    )}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>{t(`skills.activity.reason.${entry.reason}`)}</span>
                    {entry.detailCode !== "none" && (
                      <span>
                        {t(`skills.activity.detail.${entry.detailCode}`)}
                      </span>
                    )}
                    <span>{t(`skills.activity.actor.${entry.actor}`)}</span>
                    <span>{t(`skills.activity.trigger.${entry.trigger}`)}</span>
                  </div>
                  <div className="mt-1">{renderTarget(entry)}</div>
                </article>
              ))}
            </div>
          </section>
        ))}
      </div>

      {activity.hasNextPage && (
        <div className="mt-3 flex justify-center">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("skills.activity.loadMore")}
            disabled={activity.isFetchingNextPage}
            onClick={() => void activity.fetchNextPage()}
          >
            {activity.isFetchingNextPage && (
              <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            )}
            {t("skills.activity.loadMore")}
          </Button>
        </div>
      )}
    </section>
  );
}
