import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PropsWithChildren,
} from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ArchiveRestore,
  FolderOpen,
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  useAcknowledgeSkillsMigrationReport,
  useApplySkillsMigration,
  useRestoreSkillsMigrationBackup,
  useRevealSkillsMigrationFinding,
  useRevealSkillsMigrationPlanItem,
  useResumeSkillsMigration,
  useSkillsMigrationReport,
  useSkillsMigrationPreflight,
} from "@/hooks/useSkills";
import { SkillsMigrationReportBanner } from "@/components/skills/SkillsMigrationReport";
import type {
  SkillsMigrationExecutionResult,
  SkillsMigrationInventoryItem,
  SkillsMigrationItemResult,
  SkillsMigrationPlanItem,
} from "@/lib/api/skills";

interface SkillsMigrationGateProps extends PropsWithChildren {
  deferredToken?: string | null;
  enabled: boolean;
  onDefer?: (observationToken: string | null) => void;
  onReadOnlyChange?: (readOnly: boolean) => void;
}

const blockingExecutionOutcomes: ReadonlySet<
  SkillsMigrationExecutionResult["outcome"]
> = new Set(["stale_observation", "resumable", "blocked", "recovery_required"]);

function executionBlocksSkills(
  execution?: SkillsMigrationExecutionResult | null,
) {
  return Boolean(execution && blockingExecutionOutcomes.has(execution.outcome));
}

export function SkillsMigrationGate({
  children,
  deferredToken,
  enabled,
  onDefer,
  onReadOnlyChange,
}: SkillsMigrationGateProps) {
  const { t } = useTranslation();
  const preflight = useSkillsMigrationPreflight({ enabled });
  const reportQuery = useSkillsMigrationReport({ enabled });
  const applyMigration = useApplySkillsMigration();
  const acknowledgeReport = useAcknowledgeSkillsMigrationReport();
  const resumeMigration = useResumeSkillsMigration();
  const restoreMigration = useRestoreSkillsMigrationBackup();
  const revealFinding = useRevealSkillsMigrationFinding();
  const revealMigrationPlanItem = useRevealSkillsMigrationPlanItem();
  const [drift, setDrift] = useState<"unchanged" | "changed" | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [
    preserveUnsupportedConsumerFiles,
    setPreserveUnsupportedConsumerFiles,
  ] = useState(false);
  const [transientExecution, setTransientExecution] =
    useState<SkillsMigrationExecutionResult | null>(null);
  const observedToken = useRef<string | null>(null);
  const effectiveExecution = preflight.data?.execution ?? transientExecution;
  const deferred = Boolean(
    preflight.data &&
      deferredToken &&
      deferredToken === preflight.data.observationToken,
  );
  const writable =
    !enabled ||
    (!preflight.isFetching &&
      !preflight.isError &&
      preflight.data?.status === "not_required" &&
      preflight.data.pageMode === "writable" &&
      !deferred &&
      !executionBlocksSkills(effectiveExecution));

  useLayoutEffect(() => {
    onReadOnlyChange?.(!writable);
    return () => onReadOnlyChange?.(false);
  }, [onReadOnlyChange, writable]);

  useEffect(() => {
    const nextToken = preflight.data?.observationToken;
    if (!nextToken) return;
    if (observedToken.current && observedToken.current !== nextToken) {
      setDrift("changed");
      setTransientExecution(null);
      setConfirming(false);
      setPreserveUnsupportedConsumerFiles(false);
    }
    observedToken.current = nextToken;
  }, [preflight.data?.observationToken]);

  if (!enabled) {
    return children;
  }

  if (preflight.isLoading && !preflight.data) {
    return (
      <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("skills.migration.loading")}
      </div>
    );
  }

  if (preflight.isError || !preflight.data) {
    return (
      <div className="flex h-full items-center justify-center px-6">
        <div className="max-w-xl rounded-xl border border-destructive/50 bg-destructive/10 p-6 text-center">
          <AlertTriangle className="mx-auto mb-3 h-8 w-8 text-destructive" />
          <h2 className="font-semibold">{t("skills.migration.loadError")}</h2>
          <Button
            className="mt-4"
            variant="outline"
            onClick={() => void preflight.refetch()}
          >
            <RefreshCw className="mr-2 h-4 w-4" />
            {t("skills.migration.recheck")}
          </Button>
        </div>
      </div>
    );
  }

  const { inventory, plan, backup, status } = preflight.data;
  const execution = effectiveExecution;
  const report = reportQuery.data ?? null;
  const displayStatus =
    status === "not_required" && executionBlocksSkills(execution)
      ? "blocked"
      : status;
  const pendingOperation = applyMigration.isPending
    ? "applying"
    : resumeMigration.isPending
      ? "resuming"
      : restoreMigration.isPending
        ? "restoring"
        : null;
  const mutationError =
    applyMigration.error ?? resumeMigration.error ?? restoreMigration.error;
  const recheck = async () => {
    const before = preflight.data?.observationToken;
    const result = await preflight.refetch();
    const after = result.data?.observationToken;
    if (before && after) setDrift(before === after ? "unchanged" : "changed");
  };
  const applyReviewedMigration = async () => {
    setConfirming(false);
    try {
      const result = await applyMigration.mutateAsync({
        observationToken: preflight.data.observationToken,
        preserveUnsupportedConsumerFiles,
      });
      setTransientExecution(result);
      await preflight.refetch();
    } catch {
      // The mutation exposes its typed error while onSettled refreshes state.
    }
  };
  const resumePersistedMigration = async () => {
    try {
      const result = await resumeMigration.mutateAsync();
      setTransientExecution(result);
      await preflight.refetch();
    } catch {
      // The mutation exposes its typed error while onSettled refreshes state.
    }
  };
  const restorePersistedBackup = async (backupId: string) => {
    try {
      const result = await restoreMigration.mutateAsync(backupId);
      setTransientExecution(result);
      await preflight.refetch();
    } catch {
      // The mutation exposes its typed error while onSettled refreshes state.
    }
  };
  const acknowledgeMigrationReport = () => {
    if (!report?.runId || acknowledgeReport.isPending) return;
    void acknowledgeReport.mutateAsync(report.runId).catch(() => undefined);
  };
  const revealMigrationFinding = (findingId: string) => {
    if (!findingId || revealFinding.isPending) return;
    void revealFinding.mutateAsync(findingId).catch(() => undefined);
  };

  if (writable) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        {report && (
          <SkillsMigrationReportBanner
            report={report}
            acknowledgePending={acknowledgeReport.isPending}
            acknowledgeError={acknowledgeReport.error}
            onAcknowledge={acknowledgeMigrationReport}
            onReveal={revealMigrationFinding}
            revealPending={revealFinding.isPending}
            revealError={revealFinding.error}
            onRestore={
              report.backup?.restoreAvailable
                ? () => void restorePersistedBackup(report.backup!.backupId)
                : undefined
            }
            restorePending={restoreMigration.isPending}
            restoreError={restoreMigration.error}
          />
        )}
        <div className="min-h-0 flex-1">{children}</div>
      </div>
    );
  }

  if (
    deferred &&
    status === "decision_needed" &&
    !executionBlocksSkills(execution)
  ) {
    return (
      <div
        className="flex h-full min-h-0 flex-col"
        aria-readonly="true"
        data-skills-migration-readonly="true"
      >
        <div className="mx-5 mt-4 rounded-xl border border-amber-500/40 bg-amber-500/10 px-4 py-3">
          <div className="flex flex-wrap items-center gap-3">
            <ArchiveRestore className="h-5 w-5 shrink-0 text-amber-600" />
            <p className="min-w-0 flex-1 text-sm">
              {t("skills.migration.deferredDescription")}
            </p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => onDefer?.(null)}
            >
              {t("skills.migration.continueReview")}
            </Button>
          </div>
        </div>
        <div className="min-h-0 flex-1">{children}</div>
      </div>
    );
  }

  const executionOwnsActions = Boolean(execution);
  const requiresUnsupportedConsumerConsent = plan.some(
    (item) => item.disposition === "preserve_with_consent",
  );
  const hasUserResolve = plan.some(
    (item) => item.disposition === "user_resolve",
  );
  const restorableBackup = execution?.backup?.restoreAvailable
    ? execution.backup
    : undefined;
  const restoreAvailable = Boolean(restorableBackup);
  const canResumeExecution =
    execution?.outcome === "resumable" ||
    (execution?.outcome === "blocked" && restoreAvailable);
  const canRestoreExecution =
    restoreAvailable &&
    (execution?.outcome === "resumable" ||
      execution?.outcome === "blocked" ||
      execution?.outcome === "recovery_required");
  const canApply =
    status === "decision_needed" &&
    backup.ready &&
    !hasUserResolve &&
    !deferred &&
    !executionOwnsActions;

  return (
    <div className="h-full overflow-y-auto px-5 py-5">
      <div className="mx-auto max-w-5xl space-y-4">
        <section className="rounded-xl border bg-card p-5 shadow-sm">
          <div className="flex flex-wrap items-start gap-3">
            <ArchiveRestore className="mt-0.5 h-6 w-6 text-primary" />
            <div className="min-w-0 flex-1">
              <h2 className="text-lg font-semibold">
                {t("skills.migration.title")}
              </h2>
              <p className="mt-1 text-sm text-muted-foreground">
                {t("skills.migration.description")}
              </p>
            </div>
            <Badge variant="secondary">
              {t(`skills.migration.status.${displayStatus}`)}
            </Badge>
          </div>

          {deferred && (
            <p
              role="status"
              className="mt-4 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm"
            >
              {t("skills.migration.deferredDescription")}
            </p>
          )}
          {drift && (
            <p
              role={drift === "changed" ? "alert" : "status"}
              className="mt-4 rounded-md border px-3 py-2 text-sm"
            >
              {t(`skills.migration.drift.${drift}`)}
            </p>
          )}
          {pendingOperation && (
            <p
              role="status"
              className="mt-4 rounded-md border border-primary/40 bg-primary/10 px-3 py-2 text-sm"
            >
              <Loader2 className="mr-2 inline h-4 w-4 animate-spin" />
              {t(`skills.migration.pending.${pendingOperation}`)}
            </p>
          )}
          {mutationError && (
            <p
              role="alert"
              className="mt-4 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            >
              {t("skills.migration.execution.commandError")}
            </p>
          )}
          {execution && <ExecutionSummary execution={execution} />}

          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={preflight.isFetching || Boolean(pendingOperation)}
              onClick={() => void recheck()}
            >
              {preflight.isFetching ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 h-4 w-4" />
              )}
              {t("skills.migration.recheck")}
            </Button>
            {canApply && (
              <Button
                disabled={Boolean(pendingOperation)}
                onClick={() => setConfirming(true)}
              >
                {t("skills.migration.apply")}
              </Button>
            )}
            {canResumeExecution && (
              <Button
                disabled={Boolean(pendingOperation)}
                onClick={() => void resumePersistedMigration()}
              >
                {resumeMigration.isPending && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.migration.resume")}
              </Button>
            )}
            {canRestoreExecution && restorableBackup && (
              <Button
                variant="destructive"
                disabled={Boolean(pendingOperation)}
                onClick={() =>
                  void restorePersistedBackup(restorableBackup.backupId)
                }
              >
                {restoreMigration.isPending && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.migration.restore")}
              </Button>
            )}
            {status === "decision_needed" &&
              !deferred &&
              !executionOwnsActions && (
                <Button
                  variant="secondary"
                  onClick={() => onDefer?.(preflight.data.observationToken)}
                >
                  {t("skills.migration.defer")}
                </Button>
              )}
          </div>
        </section>

        <section className="rounded-xl border bg-card p-5">
          <h3 className="font-semibold">{t("skills.migration.inventory")}</h3>
          <div className="mt-3 space-y-2">
            {inventory.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("skills.migration.inventoryEmpty")}
              </p>
            ) : (
              inventory.map((item, index) => (
                <InventoryRow
                  key={`${item.kind}-${item.location}-${index}`}
                  item={item}
                />
              ))
            )}
          </div>
        </section>

        <section className="rounded-xl border bg-card p-5">
          <h3 className="font-semibold">{t("skills.migration.plan")}</h3>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("skills.migration.planDescription")}
          </p>
          <div className="mt-3 space-y-2">
            {plan.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("skills.migration.planEmpty")}
              </p>
            ) : (
              plan.map((item, index) => (
                <PlanRow
                  key={`${item.action}-${item.directory ?? "scope"}-${index}`}
                  item={item}
                  onReveal={
                    item.disposition === "user_resolve" &&
                    item.action !== "preserve_unsupported_consumer_files" &&
                    (item.fromLocation || item.toLocation)
                      ? () =>
                          revealMigrationPlanItem.mutateAsync({
                            observationToken: preflight.data.observationToken,
                            planIndex: index,
                          })
                      : undefined
                  }
                  revealPending={revealMigrationPlanItem.isPending}
                />
              ))
            )}
          </div>
          {revealMigrationPlanItem.isError && (
            <p role="alert" className="mt-3 text-sm text-destructive">
              {t("skills.migration.revealError")}
            </p>
          )}
        </section>

        <section className="rounded-xl border bg-card p-5">
          <div className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5 text-primary" />
            <h3 className="font-semibold">
              {t("skills.migration.backup.title")}
            </h3>
          </div>
          <p className="mt-2 text-sm">
            {t(
              backup.required
                ? backup.ready
                  ? "skills.migration.backup.ready"
                  : "skills.migration.backup.notReady"
                : "skills.migration.backup.notRequired",
            )}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            {t(
              backup.recoveryAvailable
                ? "skills.migration.backup.recoveryAvailable"
                : "skills.migration.backup.recoveryUnavailable",
            )}
          </p>
          {backup.databasePath && (
            <p className="mt-2 break-all font-mono text-xs">
              {backup.databasePath}
            </p>
          )}
          {backup.contentPaths.map((path) => (
            <p key={path} className="mt-1 break-all font-mono text-xs">
              {path}
            </p>
          ))}
          {!backup.ready && status === "decision_needed" && (
            <p className="mt-3 text-xs text-muted-foreground">
              {t("skills.migration.applyUnavailable")}
            </p>
          )}
        </section>
      </div>
      <Dialog open={confirming} onOpenChange={setConfirming}>
        <DialogContent zIndex="alert">
          <DialogHeader>
            <DialogTitle>{t("skills.migration.confirm.title")}</DialogTitle>
            <DialogDescription>
              {t("skills.migration.confirm.description")}
            </DialogDescription>
          </DialogHeader>
          {requiresUnsupportedConsumerConsent && (
            <label
              htmlFor="preserve-unsupported-consumer-files"
              className="flex items-start gap-3 rounded-md border p-3 text-sm"
            >
              <Checkbox
                id="preserve-unsupported-consumer-files"
                checked={preserveUnsupportedConsumerFiles}
                onCheckedChange={(checked) =>
                  setPreserveUnsupportedConsumerFiles(checked === true)
                }
              />
              <span>
                {t("skills.migration.confirm.preserveUnsupportedConsumers")}
              </span>
            </label>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              disabled={
                requiresUnsupportedConsumerConsent &&
                !preserveUnsupportedConsumerFiles
              }
              onClick={() => void applyReviewedMigration()}
            >
              {t("skills.migration.confirm.apply")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function ExecutionSummary({
  execution,
}: {
  execution: SkillsMigrationExecutionResult;
}) {
  const { t } = useTranslation();
  const highVisibility = ["blocked", "recovery_required"].includes(
    execution.outcome,
  );
  return (
    <div
      role={
        highVisibility || execution.outcome === "stale_observation"
          ? "alert"
          : "status"
      }
      className={`mt-4 rounded-md border px-3 py-3 text-sm ${
        highVisibility
          ? "border-destructive/50 bg-destructive/10 text-destructive"
          : "bg-muted/30"
      }`}
    >
      <p className="font-medium">
        {t(`skills.migration.execution.${execution.outcome}`)}
      </p>
      {execution.outcome === "completed" && (
        <p className="mt-1 text-muted-foreground">
          {t("skills.migration.execution.awaitingPreview")}
        </p>
      )}
      {execution.outcome === "blocked" &&
        !execution.backup?.restoreAvailable && (
          <p className="mt-1 text-muted-foreground">
            {t("skills.migration.execution.blockedNoBackup")}
          </p>
        )}
      <p className="mt-1 text-muted-foreground">
        {t("skills.migration.execution.progress")}:{" "}
        <span>{`${execution.progress.completedItems} / ${execution.progress.totalItems}`}</span>
      </p>
      {execution.items.length > 0 && (
        <div className="mt-3 space-y-2">
          {execution.items.map((item, index) => (
            <ExecutionItem
              key={`${item.action}-${item.directory ?? item.consumer ?? "item"}-${index}`}
              item={item}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ExecutionItem({ item }: { item: SkillsMigrationItemResult }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-md border bg-background/60 p-2">
      <div className="flex flex-wrap gap-2">
        <span>{t(`skills.migration.action.${item.action}`)}</span>
        <Badge variant="outline">
          {t(`skills.migration.itemOutcome.${item.outcome}`)}
        </Badge>
        {item.directory && (
          <span className="min-w-0 break-all font-mono">{item.directory}</span>
        )}
        {item.consumer && (
          <span className="uppercase">
            {item.consumer === "claude"
              ? t("skills.library.consumerClaude")
              : item.consumer === "codex"
                ? t("skills.library.consumerCodex")
                : item.consumer}
          </span>
        )}
      </div>
      {item.reason && (
        <p className="mt-1 text-xs text-muted-foreground">
          {t(`skills.migration.reason.${item.reason}`)}
        </p>
      )}
    </div>
  );
}

function InventoryRow({ item }: { item: SkillsMigrationInventoryItem }) {
  const { t } = useTranslation();
  return (
    <div
      className="rounded-lg border p-3"
      data-testid="migration-inventory-item"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline">
          {t(`skills.migration.kind.${item.kind}`)}
        </Badge>
        <Badge variant="secondary">
          {t(`skills.migration.inventoryState.${item.state}`)}
        </Badge>
        {item.directory && (
          <span className="min-w-0 break-all font-medium">
            {item.directory}
          </span>
        )}
        {item.consumer && (
          <span className="text-xs uppercase text-muted-foreground">
            {item.consumer === "claude"
              ? t("skills.library.consumerClaude")
              : item.consumer === "codex"
                ? t("skills.library.consumerCodex")
                : item.consumer}
          </span>
        )}
        {item.enabled !== undefined && (
          <span className="text-xs text-muted-foreground">
            {t(
              item.enabled
                ? "skills.migration.enabled"
                : "skills.migration.disabled",
            )}
          </span>
        )}
      </div>
      <p className="mt-2 break-all font-mono text-xs text-muted-foreground">
        {item.location}
      </p>
    </div>
  );
}

function PlanRow({
  item,
  onReveal,
  revealPending,
}: {
  item: SkillsMigrationPlanItem;
  onReveal?: () => Promise<boolean>;
  revealPending: boolean;
}) {
  const { t } = useTranslation();
  const isUserResolve = item.disposition === "user_resolve";
  const isConsent = item.disposition === "preserve_with_consent";
  return (
    <div className="rounded-lg border p-3" data-testid="migration-plan-item">
      <div className="flex flex-wrap items-center gap-2">
        <Badge
          variant={
            isUserResolve ? "destructive" : isConsent ? "secondary" : "outline"
          }
          className={
            isConsent
              ? "border-amber-500/40 text-amber-800 dark:text-amber-200"
              : undefined
          }
        >
          {t(`skills.migration.disposition.${item.disposition}`)}
        </Badge>
        <span className="font-medium">
          {t(`skills.migration.action.${item.action}`)}
        </span>
        {item.directory && (
          <span className="min-w-0 break-all font-mono text-xs">
            {item.directory}
          </span>
        )}
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        {t(`skills.migration.reason.${item.reason}`)}
      </p>
      {(isUserResolve || isConsent) && (
        <p className="mt-2 text-sm">
          {t(`skills.migration.guidance.${item.reason}`)}
        </p>
      )}
      {item.unsupportedConsumers && item.unsupportedConsumers.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {item.unsupportedConsumers.map((consumer) => (
            <Badge key={consumer} variant="secondary">
              {t(`skills.migration.legacyConsumer.${consumer}`)}
            </Badge>
          ))}
        </div>
      )}
      {(item.fromLocation || item.toLocation) && (
        <p className="mt-2 break-all font-mono text-xs text-muted-foreground">
          {[item.fromLocation, item.toLocation].filter(Boolean).join(" → ")}
        </p>
      )}
      {onReveal && (
        <Button
          className="mt-3"
          variant="outline"
          size="sm"
          disabled={revealPending}
          onClick={() => void onReveal().catch(() => undefined)}
        >
          {revealPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <FolderOpen className="mr-2 h-4 w-4" />
          )}
          {t("skills.migration.revealInFinder")}
        </Button>
      )}
    </div>
  );
}
