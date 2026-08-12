import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, RotateCcw } from "lucide-react";

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
  useApplySkillDeployments,
  useDeploymentRecovery,
} from "@/hooks/useSkills";
import {
  deploymentOutcomeLabelKeys,
  highVisibilityDeploymentOutcomes,
} from "@/lib/api/skills";
import type {
  DeploymentBatchResult,
  DeploymentRecoveryFinding,
  DeploymentRecoveryQuery,
  DeploymentTarget,
} from "@/lib/api/skills";

interface DeploymentRecoveryPanelProps {
  query?: DeploymentRecoveryQuery;
  onBusyChange?: (busy: boolean) => void;
}

const findingKey = (finding: DeploymentRecoveryFinding, index: number) =>
  [
    finding.target.workspace,
    finding.target.workspaceId ?? "global",
    finding.target.consumer,
    finding.entryName,
    finding.librarySkillId ?? "rejected",
    index,
  ].join(":");

const isRecoverable = (
  finding: DeploymentRecoveryFinding,
): finding is DeploymentRecoveryFinding & {
  librarySkillId: string;
  observationToken: string;
  safeReason: "exact_library_link";
} =>
  finding.disposition === "recoverable" &&
  finding.safeReason === "exact_library_link" &&
  Boolean(finding.librarySkillId) &&
  Boolean(finding.observationToken);

export function DeploymentRecoveryPanel({
  query,
  onBusyChange,
}: DeploymentRecoveryPanelProps) {
  const { t } = useTranslation();
  const recovery = useDeploymentRecovery(query);
  const apply = useApplySkillDeployments();
  const findings = recovery.data?.findings ?? [];
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [result, setResult] = useState<DeploymentBatchResult | null>(null);
  const [applyFailed, setApplyFailed] = useState(false);

  const indexedFindings = useMemo(
    () => findings.map((finding, index) => ({ finding, index })),
    [findings],
  );
  const recoverableFindings = indexedFindings.filter(({ finding }) =>
    isRecoverable(finding),
  );
  const rejectedFindings = indexedFindings.filter(
    ({ finding }) => !isRecoverable(finding),
  );
  const selectedFindings = recoverableFindings.filter(({ finding, index }) =>
    selectedKeys.has(findingKey(finding, index)),
  );
  const busy = apply.isPending || confirmOpen;

  useEffect(() => {
    onBusyChange?.(busy);
    return () => onBusyChange?.(false);
  }, [busy, onBusyChange]);

  useEffect(() => {
    // A reconciliation may issue new observation tokens. Never carry an old
    // selection across it, while preserving the previous ordered result.
    if (recovery.isFetching && !apply.isPending) {
      setSelectedKeys(new Set());
      setConfirmOpen(false);
    }
  }, [apply.isPending, recovery.isFetching]);

  const targetLabel = (target: DeploymentTarget) =>
    target.workspace === "global"
      ? t("skills.recovery.globalTarget", { consumer: target.consumer })
      : t("skills.recovery.projectTarget", {
          consumer: target.consumer,
          workspaceId: target.workspaceId ?? "",
        });

  const toggleFinding = (
    finding: DeploymentRecoveryFinding,
    index: number,
    checked: boolean,
  ) => {
    const key = findingKey(finding, index);
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
    setResult(null);
    setApplyFailed(false);
  };

  const submit = async () => {
    if (selectedFindings.length === 0 || apply.isPending) return;
    setApplyFailed(false);
    try {
      const next = await apply.mutateAsync({
        intents: selectedFindings.map(({ finding }) => ({
          action: "recover" as const,
          librarySkillId: finding.librarySkillId!,
          target: finding.target,
          observationToken: finding.observationToken!,
          confirmed: true,
        })),
      });
      setResult(next);
      setSelectedKeys(new Set());
    } catch {
      setApplyFailed(true);
    } finally {
      setConfirmOpen(false);
    }
  };

  const refresh = async () => {
    setSelectedKeys(new Set());
    setConfirmOpen(false);
    await recovery.refetch();
  };

  const renderFinding = (
    finding: DeploymentRecoveryFinding,
    index: number,
    selectable: boolean,
  ) => (
    <li
      key={findingKey(finding, index)}
      className="rounded-md border p-3"
      data-testid={`recovery-finding-${index}`}
    >
      <div className="flex items-start gap-3">
        {selectable && (
          <Checkbox
            checked={selectedKeys.has(findingKey(finding, index))}
            disabled={apply.isPending || recovery.isFetching}
            onCheckedChange={(value) =>
              toggleFinding(finding, index, value === true)
            }
            aria-label={t("skills.recovery.select", {
              entryName: finding.entryName,
            })}
          />
        )}
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{finding.entryName}</span>
            <Badge variant={selectable ? "secondary" : "outline"}>
              {selectable
                ? t(`skills.recovery.reason.${finding.safeReason}`)
                : t(`skills.recovery.disposition.${finding.disposition}`)}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground">
            {targetLabel(finding.target)}
          </p>
          {finding.librarySkillId && (
            <p className="break-all font-mono text-xs">
              {finding.librarySkillId}
            </p>
          )}
          {finding.libraryDirectory && (
            <p className="break-all text-xs text-muted-foreground">
              {t("skills.recovery.libraryDirectory", {
                directory: finding.libraryDirectory,
              })}
            </p>
          )}
          {finding.observedTarget && (
            <p className="break-all font-mono text-xs text-muted-foreground">
              {t("skills.recovery.observedTarget", {
                target: finding.observedTarget,
              })}
            </p>
          )}
          {!selectable &&
            ["archived_workspace", "unavailable_workspace"].includes(
              finding.disposition,
            ) && (
              <p className="text-xs text-destructive">
                {t("skills.recovery.restoreWorkspace")}
              </p>
            )}
        </div>
      </div>
    </li>
  );

  return (
    <section className="space-y-3 rounded-xl border bg-muted/20 p-4">
      <div className="flex flex-wrap items-start gap-3">
        <RotateCcw className="mt-0.5 h-4 w-4 text-primary" />
        <div className="min-w-0 flex-1">
          <h3 className="font-semibold">{t("skills.recovery.title")}</h3>
          <p className="text-xs text-muted-foreground">
            {t("skills.recovery.description")}
          </p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          disabled={recovery.isFetching || apply.isPending || confirmOpen}
          onClick={() => void refresh()}
          aria-label={t("skills.recovery.refresh")}
        >
          <RefreshCw
            className={`mr-1 h-3.5 w-3.5${recovery.isFetching ? " animate-spin" : ""}`}
          />
          {t("skills.recovery.refresh")}
        </Button>
      </div>

      {recovery.isLoading && (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("skills.recovery.loading")}
        </p>
      )}
      {recovery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {t("skills.recovery.loadError")}
        </p>
      )}
      {!recovery.isLoading && !recovery.isError && findings.length === 0 && (
        <p className="text-sm text-muted-foreground">
          {t("skills.recovery.empty")}
        </p>
      )}

      {recoverableFindings.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-sm font-medium">
            {t("skills.recovery.safeTitle")}
          </h4>
          <ol className="space-y-2">
            {recoverableFindings.map(({ finding, index }) =>
              renderFinding(finding, index, true),
            )}
          </ol>
        </div>
      )}

      {rejectedFindings.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-sm font-medium">
            {t("skills.recovery.rejectedTitle")}
          </h4>
          <ol className="space-y-2">
            {rejectedFindings.map(({ finding, index }) =>
              renderFinding(finding, index, false),
            )}
          </ol>
        </div>
      )}

      {applyFailed && (
        <p role="alert" className="text-sm text-destructive">
          {t("skills.recovery.applyError")}
        </p>
      )}

      {result && (
        <div className="space-y-2 rounded-md border bg-background p-3">
          <h4 className="text-sm font-medium">
            {t("skills.recovery.results")}
          </h4>
          <ol className="list-decimal space-y-1 pl-5 text-sm">
            {result.items.map((item, index) => (
              <li
                key={`${item.librarySkillId}-${index}`}
                data-testid={`recovery-result-${index}`}
                role={
                  highVisibilityDeploymentOutcomes.has(item.outcome)
                    ? "alert"
                    : undefined
                }
              >
                <span className="font-mono text-xs">{item.librarySkillId}</span>{" "}
                · {targetLabel(item.target)} ·{" "}
                <Badge
                  variant={
                    highVisibilityDeploymentOutcomes.has(item.outcome)
                      ? "destructive"
                      : "secondary"
                  }
                >
                  {t(deploymentOutcomeLabelKeys[item.outcome])}
                </Badge>
              </li>
            ))}
          </ol>
        </div>
      )}

      <Button
        disabled={
          selectedFindings.length === 0 ||
          apply.isPending ||
          recovery.isFetching
        }
        onClick={() => setConfirmOpen(true)}
      >
        {t("skills.recovery.reviewSelected")}
      </Button>

      <Dialog
        open={confirmOpen}
        onOpenChange={(open) => {
          if (!apply.isPending) setConfirmOpen(open);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("skills.recovery.confirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.recovery.confirmDescription", {
                count: selectedFindings.length,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={apply.isPending}
              onClick={() => setConfirmOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button disabled={apply.isPending} onClick={() => void submit()}>
              {apply.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t("skills.recovery.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
