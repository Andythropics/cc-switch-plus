import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import type { DeploymentStatus, ObservedDeployment } from "@/lib/api/skills";

interface DeploymentStatusBadgeProps {
  status: DeploymentStatus;
  observed?: ObservedDeployment;
  desired: boolean;
  className?: string;
}

const statusClasses: Record<DeploymentStatus, string> = {
  not_deployed: "border-border text-muted-foreground",
  in_sync:
    "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  drift:
    "border-amber-500/50 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  conflict: "border-red-500/50 bg-red-500/10 text-red-700 dark:text-red-300",
  orphaned:
    "border-orange-500/50 bg-orange-500/10 text-orange-700 dark:text-orange-300",
  blocked:
    "border-slate-500/50 bg-slate-500/10 text-slate-700 dark:text-slate-300",
  archived:
    "border-violet-500/50 bg-violet-500/10 text-violet-700 dark:text-violet-300",
  unsupported:
    "border-slate-500/50 bg-slate-500/10 text-slate-700 dark:text-slate-300",
};

/**
 * Render the derived deployment status alongside the raw desired/observed
 * facts. Keeping both visible prevents a database intent from being mistaken
 * for a working filesystem link.
 */
export function DeploymentStatusBadge({
  status,
  observed,
  desired,
  className,
}: DeploymentStatusBadgeProps) {
  const { t } = useTranslation();
  const observedState = observed?.state ?? "missing";
  const details = [
    observed?.targetPath &&
      `${t("skills.library.targetPath")}: ${observed.targetPath}`,
    observed?.expectedTarget &&
      `${t("skills.library.expectedTarget")}: ${observed.expectedTarget}`,
    observed?.actualTarget &&
      `${t("skills.library.actualTarget")}: ${observed.actualTarget}`,
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <div
      className={`flex min-w-0 flex-wrap items-center gap-1.5 ${className ?? ""}`}
      data-testid={`deployment-status-${status}`}
      title={details || undefined}
    >
      <Badge variant="outline" className={statusClasses[status]}>
        {t(`skills.library.deploymentStatus.${status}`)}
      </Badge>
      <Badge variant="outline" className="font-normal text-muted-foreground">
        {t("skills.library.observed")}:{" "}
        {t(`skills.library.observedState.${observedState}`)}
      </Badge>
      <span className="text-[11px] text-muted-foreground">
        {desired
          ? t("skills.library.desiredRecorded")
          : t("skills.library.desiredNotRecorded")}
      </span>
    </div>
  );
}
