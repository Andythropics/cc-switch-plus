import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import type { DeploymentStatus, ObservedDeployment } from "@/lib/api/skills";

interface DeploymentStatusBadgeProps {
  status: DeploymentStatus;
  observed?: ObservedDeployment;
  className?: string;
}

const deployedClasses =
  "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
const notDeployedClasses = "border-border text-muted-foreground";

/**
 * Render the binary user-facing deployment state while using observed facts
 * to prefer the filesystem truth over rare backend status variants.
 */
export function DeploymentStatusBadge({
  status,
  observed,
  className,
}: DeploymentStatusBadgeProps) {
  const { t } = useTranslation();
  const isDeployed =
    status === "in_sync" ||
    observed?.state === "correct_link" ||
    observed?.state === "unrecorded_link";

  return (
    <div
      className={`flex min-w-0 flex-wrap items-center gap-1.5 ${className ?? ""}`}
      data-testid={`deployment-status-${status}`}
    >
      <Badge
        variant="outline"
        className={isDeployed ? deployedClasses : notDeployedClasses}
      >
        {t(
          `skills.library.deploymentStatus.${isDeployed ? "deployed" : "not_deployed"}`,
        )}
      </Badge>
    </div>
  );
}
