import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Database, Link2, Unlink, Wrench } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
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
import type { WorkspaceLifecycle } from "@/lib/api/projectWorkspaces";

interface DeploymentResolutionActionsProps {
  skill: LibrarySkill;
  target: DeploymentTarget;
  deployment?: DeploymentInspection;
  compatible: boolean;
  workspaceLifecycle?: WorkspaceLifecycle;
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
  workspaceLifecycle = "active",
  disabled = false,
  isPending = false,
  deployLabel,
  undeployLabel,
  onApply,
}: DeploymentResolutionActionsProps) {
  const { t } = useTranslation();
  const [replaceDialogOpen, setReplaceDialogOpen] = useState(false);
  const [forgetDialogOpen, setForgetDialogOpen] = useState(false);
  const [undeployDialogOpen, setUndeployDialogOpen] = useState(false);
  const [undeployPending, setUndeployPending] = useState(false);

  const status = deployment?.status ?? "not_deployed";
  const hasDesired = Boolean(deployment?.desired);
  const isLifecycleBlocked = lifecycleBlockedStatuses.has(status);
  const isUnsupported = status === "unsupported";
  const filesystemUnavailable = workspaceLifecycle === "unavailable";
  const lifecyclePreventsFilesystem =
    workspaceLifecycle !== "active" || isLifecycleBlocked;
  const actionsDisabled = disabled || isPending;
  const observationToken = deployment?.observationToken;
  const observedState = deployment?.observed.state;

  // The backend's safe repair contract currently accepts a missing target.
  // Foreign links are handled by the explicit replacement flow below.
  const canRepair =
    hasDesired &&
    workspaceLifecycle === "active" &&
    status === "drift" &&
    observedState === "missing" &&
    Boolean(observationToken);
  const canReplaceForeignLink =
    hasDesired &&
    workspaceLifecycle === "active" &&
    (status === "conflict" || status === "drift") &&
    (observedState === "redirected_link" ||
      observedState === "broken_link" ||
      observedState === "invalid_link") &&
    Boolean(observationToken);
  // DB-only cleanup is safe for every recorded, out-of-sync deployment. The
  // backend still guards the mutation, while unsupported targets remain
  // visible-but-disabled so the user can see why they cannot be resolved.
  const canForget = hasDesired && status !== "in_sync";
  const canUndeploy = hasDesired;
  const canDeploy = !hasDesired && status === "not_deployed";

  const apply = (intent: DeploymentIntent) => {
    void onApply(intent);
  };

  const confirmUndeploy = async () => {
    if (undeployPending) return;
    setUndeployPending(true);
    try {
      await onApply({
        action: "undeploy",
        librarySkillId: skill.id,
        target,
      });
      setUndeployDialogOpen(false);
    } finally {
      setUndeployPending(false);
    }
  };

  return (
    <>
      {canDeploy && (
        <Button
          variant="outline"
          size="sm"
          disabled={
            actionsDisabled ||
            !compatible ||
            lifecyclePreventsFilesystem ||
            isUnsupported
          }
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
          disabled={actionsDisabled || filesystemUnavailable || isUnsupported}
          title={
            status === "drift" || status === "conflict"
              ? t("skills.library.undeploySafetyDescription")
              : undefined
          }
          onClick={() => setUndeployDialogOpen(true)}
        >
          <Unlink className="mr-1.5 h-3.5 w-3.5" />
          {undeployLabel}
        </Button>
      )}

      <Dialog
        open={undeployDialogOpen}
        onOpenChange={(open) => {
          if (!undeployPending) setUndeployDialogOpen(open);
        }}
      >
        <DialogContent zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("skills.library.undeployConfirmTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("skills.library.undeployConfirmDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-3 text-sm">
            <p className="break-words font-medium">{skill.displayName}</p>
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-2 text-muted-foreground">
              <dt>{t("skills.library.undeployConsumer")}</dt>
              <dd>
                {t(
                  target.consumer === "claude"
                    ? "skills.library.consumerClaude"
                    : "skills.library.consumerCodex",
                )}
              </dd>
              <dt>{t("skills.library.undeployWorkspace")}</dt>
              <dd className="break-all">
                {target.workspace === "global"
                  ? t("skills.batch.global")
                  : t("skills.library.undeployProjectWorkspace", {
                      workspaceId: target.workspaceId ?? "",
                    })}
              </dd>
            </dl>
          </DialogBody>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={undeployPending}
              onClick={() => setUndeployDialogOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={undeployPending}
              onClick={() => void confirmUndeploy()}
            >
              {t("skills.library.undeployConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {canRepair && (
        <Button
          variant="outline"
          size="sm"
          disabled={
            actionsDisabled ||
            !compatible ||
            lifecyclePreventsFilesystem ||
            isUnsupported
          }
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
          disabled={
            actionsDisabled ||
            !compatible ||
            lifecyclePreventsFilesystem ||
            isUnsupported
          }
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
          disabled={actionsDisabled || isUnsupported}
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
