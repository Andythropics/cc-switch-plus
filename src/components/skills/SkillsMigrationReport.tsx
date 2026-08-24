import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArchiveRestore,
  ChevronDown,
  FolderOpen,
  Loader2,
  ShieldCheck,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  SkillsMigrationFinding,
  SkillsMigrationReport,
  SkillsMigrationPlanDisposition,
} from "@/lib/api/skills";

interface SkillsMigrationReportProps {
  report: SkillsMigrationReport;
  acknowledgePending?: boolean;
  acknowledgeError?: unknown;
  onAcknowledge?: () => void;
  onReveal?: (findingId: string) => void;
  revealPending?: boolean;
  revealError?: unknown;
  onRestore?: () => void;
  restorePending?: boolean;
  restoreError?: unknown;
}

/**
 * The migration report is deliberately independent of the migration gate.
 * Once a run is complete the Skills page can be writable while this report
 * remains a durable, read-only audit entry at the top of the page.
 */
export function SkillsMigrationReportBanner({
  report,
  acknowledgePending = false,
  acknowledgeError,
  onAcknowledge,
  onReveal,
  revealPending = false,
  revealError,
  onRestore,
  restorePending = false,
  restoreError,
}: SkillsMigrationReportProps) {
  const { t } = useTranslation();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [restoreConfirmOpen, setRestoreConfirmOpen] = useState(false);
  const openCount = report.summary.open;
  const acknowledged = Boolean(report.acknowledgedAt);
  const hasHardOpenFindings = report.findings.some(
    (finding) =>
      finding.disposition === "user_resolve" && finding.status === "open",
  );
  const accent = acknowledged
    ? "border-border bg-muted/30"
    : hasHardOpenFindings
      ? "border-destructive/50 bg-destructive/5"
      : "border-amber-500/40 bg-amber-500/10";

  return (
    <>
      <section
        className={`mx-5 ${acknowledged ? "mt-2 px-3 py-2" : "mt-4 px-4 py-3"} rounded-xl border shadow-sm ${accent}`}
        data-testid="skills-migration-report-banner"
      >
        <div className="flex flex-wrap items-start gap-3">
          <ArchiveRestore
            className={`mt-0.5 h-5 w-5 shrink-0 ${
              acknowledged
                ? "text-muted-foreground"
                : hasHardOpenFindings
                  ? "text-destructive"
                  : "text-amber-600"
            }`}
          />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="font-semibold text-sm">
                {t("skills.migration.report.title")}
              </h2>
              {!acknowledged && (
                <Badge
                  variant={hasHardOpenFindings ? "destructive" : "secondary"}
                  className={
                    !hasHardOpenFindings
                      ? "border-amber-500/40 text-amber-800 dark:text-amber-200"
                      : undefined
                  }
                >
                  {t(`skills.migration.report.state.${report.state}`)}
                </Badge>
              )}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t(
                acknowledged
                  ? "skills.migration.report.historyEntry"
                  : "skills.migration.report.description",
              )}
            </p>
            <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs">
              <span data-testid="skills-migration-report-performed">
                {t("skills.migration.report.performed", {
                  count: report.summary.performed,
                })}
              </span>
              <span data-testid="skills-migration-report-preserved">
                {t("skills.migration.report.preserved", {
                  count: report.summary.preserved,
                })}
              </span>
              {openCount > 0 && (
                <span
                  className={
                    hasHardOpenFindings
                      ? "font-medium text-destructive"
                      : "font-medium text-amber-700 dark:text-amber-300"
                  }
                  data-testid="skills-migration-report-open"
                >
                  {t("skills.migration.report.open", { count: openCount })}
                </span>
              )}
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setDetailsOpen(true)}
            >
              <ChevronDown className="mr-2 h-4 w-4" />
              {t("skills.migration.report.details")}
            </Button>
            {!acknowledged && onAcknowledge && (
              <Button
                variant="secondary"
                size="sm"
                disabled={acknowledgePending}
                onClick={onAcknowledge}
              >
                {acknowledgePending && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.migration.report.acknowledge")}
              </Button>
            )}
          </div>
        </div>
        {acknowledged && (
          <p className="mt-2 text-xs text-muted-foreground">
            {t("skills.migration.report.acknowledged")}
          </p>
        )}
        {Boolean(acknowledgeError) && (
          <p role="alert" className="mt-2 text-xs text-destructive">
            {t("skills.migration.report.acknowledgeError")}
          </p>
        )}
      </section>

      <Dialog open={detailsOpen} onOpenChange={setDetailsOpen}>
        <DialogContent className="max-h-[85vh] overflow-y-auto" zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("skills.migration.report.detailsTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("skills.migration.report.detailsDescription")}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3">
            {report.findings.length === 0 ? (
              <p className="rounded-md border bg-muted/30 px-3 py-3 text-sm text-muted-foreground">
                {t("skills.migration.report.noFindings")}
              </p>
            ) : (
              report.findings.map((finding) => (
                <MigrationReportFinding
                  key={finding.findingId}
                  finding={finding}
                  onReveal={onReveal}
                  revealPending={revealPending}
                />
              ))
            )}
          </div>

          {Boolean(revealError) && (
            <p role="alert" className="text-sm text-destructive">
              {t("skills.migration.report.revealError")}
            </p>
          )}

          {report.backup?.restoreAvailable && report.backup.backupId && (
            <div className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-3">
              <div className="flex items-start gap-2">
                <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
                <p className="text-sm">
                  {t("skills.migration.report.verifiedBackupDescription")}
                </p>
              </div>
              {onRestore && (
                <Button
                  className="mt-3"
                  variant="outline"
                  size="sm"
                  disabled={restorePending}
                  onClick={() => setRestoreConfirmOpen(true)}
                >
                  {restorePending && (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  )}
                  <ArchiveRestore className="mr-2 h-4 w-4" />
                  {t("skills.migration.report.restore")}
                </Button>
              )}
              {Boolean(restoreError) && (
                <p role="alert" className="mt-2 text-xs text-destructive">
                  {t("skills.migration.report.restoreError")}
                </p>
              )}
            </div>
          )}

          <DialogFooter>
            <Button variant="outline" onClick={() => setDetailsOpen(false)}>
              {t("common.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={restoreConfirmOpen} onOpenChange={setRestoreConfirmOpen}>
        <DialogContent zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("skills.migration.report.restoreConfirmTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("skills.migration.report.restoreConfirmDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setRestoreConfirmOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={restorePending}
              onClick={() => {
                setRestoreConfirmOpen(false);
                onRestore?.();
              }}
            >
              {restorePending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t("skills.migration.report.restoreConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

function MigrationReportFinding({
  finding,
  onReveal,
  revealPending,
}: {
  finding: SkillsMigrationFinding;
  onReveal?: (findingId: string) => void;
  revealPending: boolean;
}) {
  const { t } = useTranslation();
  const disposition = finding.disposition as SkillsMigrationPlanDisposition;
  const isUserResolve = disposition === "user_resolve";
  const isConsent = disposition === "preserve_with_consent";
  const badgeClass = isConsent
    ? "border-amber-500/40 text-amber-800 dark:text-amber-200"
    : undefined;

  return (
    <div className="rounded-lg border bg-background/60 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge
          variant={
            isUserResolve ? "destructive" : isConsent ? "secondary" : "outline"
          }
          className={badgeClass}
        >
          {t(`skills.migration.disposition.${finding.disposition}`)}
        </Badge>
        {finding.action && (
          <span className="font-medium">
            {t(`skills.migration.action.${finding.action}`)}
          </span>
        )}
        {finding.directory && (
          <span className="font-mono text-xs">{finding.directory}</span>
        )}
        {finding.consumer && (
          <span className="text-xs uppercase text-muted-foreground">
            {finding.consumer === "claude"
              ? t("skills.library.consumerClaude")
              : finding.consumer === "codex"
                ? t("skills.library.consumerCodex")
                : finding.consumer}
          </span>
        )}
      </div>
      {finding.reason && (
        <p className="mt-1 text-xs text-muted-foreground">
          {t(`skills.migration.reason.${finding.reason}`)}
        </p>
      )}
      {(isUserResolve || isConsent) && finding.reason && (
        <p className="mt-2 text-sm">
          {t(`skills.migration.guidance.${finding.reason}`)}
        </p>
      )}
      {finding.detailComplete === false && (
        <p className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-xs text-amber-800 dark:text-amber-200">
          {t("skills.migration.report.detailIncomplete")}
        </p>
      )}
      {finding.unsupportedConsumers &&
        finding.unsupportedConsumers.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-2">
            {finding.unsupportedConsumers.map((consumer) => (
              <Badge key={consumer} variant="secondary">
                {t(`skills.migration.legacyConsumer.${consumer}`)}
              </Badge>
            ))}
          </div>
        )}
      {(finding.fromLocation || finding.toLocation) && (
        <p className="mt-2 break-all font-mono text-xs text-muted-foreground">
          {[finding.fromLocation, finding.toLocation]
            .filter(Boolean)
            .join(" → ")}
        </p>
      )}
      {onReveal &&
        finding.findingId &&
        (finding.fromLocation || finding.toLocation) && (
          <Button
            className="mt-3"
            variant="outline"
            size="sm"
            disabled={revealPending}
            onClick={() => onReveal(finding.findingId)}
          >
            {revealPending ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <FolderOpen className="mr-2 h-4 w-4" />
            )}
            {t("skills.migration.report.reveal")}
          </Button>
        )}
    </div>
  );
}
