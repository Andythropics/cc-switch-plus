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
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useSkillsMigrationPreflight } from "@/hooks/useSkills";
import type {
  SkillsMigrationInventoryItem,
  SkillsMigrationPlanItem,
} from "@/lib/api/skills";

interface SkillsMigrationGateProps extends PropsWithChildren {
  deferredToken?: string | null;
  enabled: boolean;
  onDefer?: (observationToken: string) => void;
  onReadOnlyChange?: (readOnly: boolean) => void;
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
  const [drift, setDrift] = useState<"unchanged" | "changed" | null>(null);
  const observedToken = useRef<string | null>(null);
  const writable =
    !enabled ||
    (!preflight.isFetching &&
      !preflight.isError &&
      preflight.data?.status === "not_required" &&
      preflight.data.pageMode === "writable");

  useLayoutEffect(() => {
    onReadOnlyChange?.(!writable);
    return () => onReadOnlyChange?.(false);
  }, [onReadOnlyChange, writable]);

  useEffect(() => {
    const nextToken = preflight.data?.observationToken;
    if (!nextToken) return;
    if (observedToken.current && observedToken.current !== nextToken) {
      setDrift("changed");
    }
    observedToken.current = nextToken;
  }, [preflight.data?.observationToken]);

  if (writable) {
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
  const deferred = Boolean(deferredToken);
  const recheck = async () => {
    const before = preflight.data?.observationToken;
    const result = await preflight.refetch();
    const after = result.data?.observationToken;
    if (before && after) setDrift(before === after ? "unchanged" : "changed");
  };

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
              {t(`skills.migration.status.${status}`)}
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

          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={preflight.isFetching}
              onClick={() => void recheck()}
            >
              {preflight.isFetching ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 h-4 w-4" />
              )}
              {t("skills.migration.recheck")}
            </Button>
            {status === "decision_needed" && !deferred && (
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
                />
              ))
            )}
          </div>
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
          <p className="mt-3 text-xs text-muted-foreground">
            {t("skills.migration.applyUnavailable")}
          </p>
        </section>
      </div>
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
          <span className="font-medium">{item.directory}</span>
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

function PlanRow({ item }: { item: SkillsMigrationPlanItem }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border p-3" data-testid="migration-plan-item">
      <div className="flex flex-wrap items-center gap-2">
        <Badge
          variant={
            item.disposition === "user_resolve" ? "destructive" : "outline"
          }
        >
          {t(`skills.migration.disposition.${item.disposition}`)}
        </Badge>
        <span className="font-medium">
          {t(`skills.migration.action.${item.action}`)}
        </span>
        {item.directory && (
          <span className="font-mono text-xs">{item.directory}</span>
        )}
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        {t(`skills.migration.reason.${item.reason}`)}
      </p>
      {(item.fromLocation || item.toLocation) && (
        <p className="mt-2 break-all font-mono text-xs text-muted-foreground">
          {[item.fromLocation, item.toLocation].filter(Boolean).join(" → ")}
        </p>
      )}
    </div>
  );
}
