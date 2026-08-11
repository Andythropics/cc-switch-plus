import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Database, Link2, Unlink, Wrench } from "lucide-react";

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
  DeploymentInspection,
  DeploymentIntent,
  DeploymentTarget,
  LibrarySkill,
} from "@/lib/api/skills";

interface DeploymentResolutionActionsProps {
  skill: LibrarySkill;
  target: DeploymentTarget;
  deployment?: DeploymentInspection;
  compatible: boolean;
  /** Disable actions while the containing panel is busy. */
  disabled?: boolean;
  isPending?: boolean;
  deployLabel: string;
  undeployLabel: string;
  onApply: (intent: DeploymentIntent) => Promise<void>;
}

const lifecycleBlockedStatuses = new Set([
  "archived",
  "blocked",
  "unsupported",
]);

/**
 * Action policy for a single observed deployment.
 *
 * Resolution actions are deliberately derived from the inspection facts. A
 * repair requires a current token and only handles a missing managed target;
 * replacing a redirected (foreign) link always goes through a confirmation
 * dialog. Undeploy remains available for drift/conflict because the backend
 * performs a non-destructive safety check before changing the filesystem.
 */
export function DeploymentResolutionActions({
  skill,
  target,
  deployment,
  compatible,
  disabled = false,
  isPending = false,
  deployLabel,
  undeployLabel,
  onApply,
}: DeploymentResolutionActionsProps) {
  const { t } = useTranslation();
  const [replaceDialogOpen, setReplaceDialogOpen] = useState(false);
  const [forgetDialogOpen, setForgetDialogOpen] = useState(false);

  const status = deployment?.status ?? "not_deployed";
  const hasDesired = Boolean(deployment?.desired);
  const isLifecycleBlocked = lifecycleBlockedStatuses.has(status);
  const actionsDisabled =
    disabled || isPending || !compatible || isLifecycleBlocked;
  const observationToken = deployment?.observationToken;
  const observedState = deployment?.observed.state;

  // The backend's safe repair contract currently accepts a missing target.
  // Foreign links are handled by the explicit replacement flow below.
  const canRepair =
    hasDesired &&
    status === "drift" &&
    observedState === "missing" &&
    Boolean(observationToken);
  const canReplaceForeignLink =
    hasDesired &&
    (status === "conflict" || status === "drift") &&
    (observedState === "redirected_link" ||
      observedState === "broken_link" ||
      observedState === "invalid_link") &&
    Boolean(observationToken);
  const canForget = hasDesired && status !== "in_sync" && !isLifecycleBlocked;
  const canUndeploy = hasDesired;
  const canDeploy = !hasDesired && status === "not_deployed";

  const apply = (intent: DeploymentIntent) => {
    void onApply(intent);
  };

  return (
    <>
      {canDeploy && (
        <Button
          variant="outline"
          size="sm"
          disabled={actionsDisabled}
          onClick={() =>
            apply({
              action: "deploy",
              librarySkillId: skill.id,
              target,
            })
          }
        >
          <Link2 className="mr-1.5 h-3.5 w-3.5" />
          {deployLabel}
        </Button>
      )}

      {canUndeploy && (
        <Button
          variant="outline"
          size="sm"
          disabled={actionsDisabled}
          title={
            status === "drift" || status === "conflict"
              ? t("skills.library.undeploySafetyDescription")
              : undefined
          }
          onClick={() =>
            apply({
              action: "undeploy",
              librarySkillId: skill.id,
              target,
            })
          }
        >
          <Unlink className="mr-1.5 h-3.5 w-3.5" />
          {undeployLabel}
        </Button>
      )}

      {canRepair && (
        <Button
          variant="outline"
          size="sm"
          disabled={actionsDisabled}
          onClick={() =>
            apply({
              action: "repair",
              librarySkillId: skill.id,
              target,
              observationToken: observationToken!,
            })
          }
        >
          <Wrench className="mr-1.5 h-3.5 w-3.5" />
          {t("skills.library.repair")}
        </Button>
      )}

      {canReplaceForeignLink && (
        <Button
          variant="destructive"
          size="sm"
          disabled={actionsDisabled}
          onClick={() => setReplaceDialogOpen(true)}
        >
          <Wrench className="mr-1.5 h-3.5 w-3.5" />
          {t("skills.library.replaceForeignLink")}
        </Button>
      )}

      {canForget && (
        <Button
          variant="ghost"
          size="sm"
          disabled={actionsDisabled}
          title={t("skills.library.forgetDescription")}
          onClick={() => setForgetDialogOpen(true)}
        >
          <Database className="mr-1.5 h-3.5 w-3.5" />
          {t("skills.library.forget")}
        </Button>
      )}

      <Dialog open={replaceDialogOpen} onOpenChange={setReplaceDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t("skills.library.replaceForeignLinkTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("skills.library.replaceForeignLinkDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setReplaceDialogOpen(false)}
            >
              {t("skills.library.resolutionCancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setReplaceDialogOpen(false);
                if (!observationToken) return;
                apply({
                  action: "replaceForeignLink",
                  librarySkillId: skill.id,
                  target,
                  observationToken,
                  confirmed: true,
                });
              }}
            >
              {t("skills.library.replaceForeignLinkConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={forgetDialogOpen} onOpenChange={setForgetDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("skills.library.forgetTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.library.forgetDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setForgetDialogOpen(false)}
            >
              {t("skills.library.resolutionCancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setForgetDialogOpen(false);
                apply({
                  action: "forget",
                  librarySkillId: skill.id,
                  target,
                });
              }}
            >
              {t("skills.library.forgetConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
