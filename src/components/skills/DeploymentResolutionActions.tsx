import { useState } from "react";
import { Portal as TooltipPortal } from "@radix-ui/react-tooltip";
import { useTranslation } from "react-i18next";
import { Database, Link2, Loader2, Unlink, Wrench } from "lucide-react";

import { ClaudeIcon, CodexIcon } from "@/components/BrandIcons";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkillsDialogContent } from "@/components/skills/SkillsDialogContent";
import type {
  DeploymentInspection,
  DeploymentIntent,
  DeploymentTarget,
  LibrarySkill,
} from "@/lib/api/skills";
import type { WorkspaceLifecycle } from "@/lib/api/projectWorkspaces";

interface DeploymentResolutionActionsProps {
  iconToggle?: boolean;
  /** Render only the stable platform toggle; resolution actions live in details. */
  toggleOnly?: boolean;
  /** Issue dialogs exclude routine deploy and undeploy controls. */
  resolutionOnly?: boolean;
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
  iconToggle = false,
  toggleOnly = false,
  resolutionOnly = false,
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
      {iconToggle && !resolutionOnly && (
        <TooltipProvider delayDuration={200}>
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <Button
                  variant="outline"
                  size="icon"
                  className={`h-9 w-9 ${status === "in_sync" ? "border-primary/40 bg-primary/10 hover:bg-primary/20" : ""}`}
                  aria-label={canUndeploy ? undeployLabel : deployLabel}
                  aria-pressed={hasDesired}
                  aria-busy={isPending}
                  disabled={
                    actionsDisabled ||
                    (canUndeploy
                      ? filesystemUnavailable || isUnsupported
                      : !canDeploy ||
                        !compatible ||
                        lifecyclePreventsFilesystem ||
                        isUnsupported)
                  }
                  onClick={() => {
                    if (canUndeploy && status !== "in_sync") {
                      setUndeployDialogOpen(true);
                    } else {
                      apply({
                        action: canUndeploy ? "undeploy" : "deploy",
                        librarySkillId: skill.id,
                        target,
                      });
                    }
                  }}
                >
                  {isPending ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : target.consumer === "claude" ? (
                    <ClaudeIcon size={18} />
                  ) : (
                    <CodexIcon size={18} />
                  )}
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipPortal>
              <TooltipContent>
                <p>{canUndeploy ? undeployLabel : deployLabel}</p>
                {!compatible && <p>{t("skills.batch.incompatible")}</p>}
                {status !== "in_sync" && status !== "not_deployed" && (
                  <p>{t(`skills.library.deploymentStatus.${status}`)}</p>
                )}
              </TooltipContent>
            </TooltipPortal>
          </Tooltip>
        </TooltipProvider>
      )}
      {!iconToggle && !resolutionOnly && canDeploy && (
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
          <Link2 className="h-4 w-4" />
          {deployLabel}
        </Button>
      )}

      {!iconToggle && !resolutionOnly && canUndeploy && (
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
          <Unlink className="h-4 w-4" />
          {undeployLabel}
        </Button>
      )}

      <Dialog
        open={undeployDialogOpen}
        onOpenChange={(open) => {
          if (!undeployPending) setUndeployDialogOpen(open);
        }}
      >
        <SkillsDialogContent zIndex="alert" closeBlocked={undeployPending}>
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
        </SkillsDialogContent>
      </Dialog>

      {!toggleOnly && canRepair && (
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
          <Wrench className="h-4 w-4" />
          {t("skills.library.repair")}
        </Button>
      )}

      {!toggleOnly && canReplaceForeignLink && (
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
          <Wrench className="h-4 w-4" />
          {t("skills.library.replaceForeignLink")}
        </Button>
      )}

      {!toggleOnly && canForget && (
        <Button
          variant="outline"
          size="sm"
          disabled={actionsDisabled || isUnsupported}
          title={t("skills.library.forgetDescription")}
          onClick={() => setForgetDialogOpen(true)}
        >
          <Database className="h-4 w-4" />
          {t("skills.library.forget")}
        </Button>
      )}

      <Dialog open={replaceDialogOpen} onOpenChange={setReplaceDialogOpen}>
        <SkillsDialogContent zIndex="alert" closeBlocked={isPending}>
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
              disabled={isPending}
              onClick={() => setReplaceDialogOpen(false)}
            >
              {t("skills.library.resolutionCancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={isPending}
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
        </SkillsDialogContent>
      </Dialog>

      <Dialog open={forgetDialogOpen} onOpenChange={setForgetDialogOpen}>
        <SkillsDialogContent zIndex="alert" closeBlocked={isPending}>
          <DialogHeader>
            <DialogTitle>{t("skills.library.forgetTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.library.forgetDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={isPending}
              onClick={() => setForgetDialogOpen(false)}
            >
              {t("skills.library.resolutionCancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={isPending}
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
        </SkillsDialogContent>
      </Dialog>
    </>
  );
}
