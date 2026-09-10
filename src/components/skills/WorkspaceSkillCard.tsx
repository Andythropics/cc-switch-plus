import { useRef, useState, type MouseEvent, type ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, CheckCircle2 } from "lucide-react";
import { ClaudeIcon, CodexIcon } from "@/components/BrandIcons";
import type { LibrarySkill } from "@/lib/api/skills";
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
import { SkillTechnicalDetails } from "@/components/skills/SkillTechnicalDetails";
import { DeploymentResolutionActions } from "@/components/skills/DeploymentResolutionActions";

export const workspaceSkillGridClassName =
  "grid grid-cols-[repeat(auto-fit,minmax(min(100%,20rem),1fr))] items-stretch gap-3";
export const sourceSummary = (skill: LibrarySkill) => {
  if (skill.source.repoOwner && skill.source.repoName)
    return `${skill.source.repoOwner}/${skill.source.repoName}`;
  return skill.source.url;
};
export type WorkspaceDeploymentControl = Omit<
  ComponentProps<typeof DeploymentResolutionActions>,
  "iconToggle" | "toggleOnly" | "resolutionOnly"
>;

function hasIssue(control: WorkspaceDeploymentControl) {
  return (
    !control.compatible ||
    Boolean(
      control.deployment &&
        !["in_sync", "not_deployed"].includes(control.deployment.status),
    )
  );
}

/** Keep platform controls independent of metadata and resolution action count. */
export function WorkspaceSkillCard({
  skill,
  controls,
}: {
  skill: LibrarySkill;
  controls: WorkspaceDeploymentControl[];
}) {
  const { t } = useTranslation();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const opener = useRef<HTMLButtonElement | null>(null);
  const card = useRef<HTMLElement | null>(null);
  const busy = controls.some((control) => control.isPending);
  const issues = controls.filter(hasIssue);
  const consumerLabel = (control: WorkspaceDeploymentControl) =>
    t(
      control.target.consumer === "claude"
        ? "skills.library.consumerClaude"
        : "skills.library.consumerCodex",
    );
  const reason = (control: WorkspaceDeploymentControl) => {
    if (control.workspaceLifecycle === "unavailable")
      return t("skills.workspaceCard.unavailable");
    if (control.workspaceLifecycle === "archived")
      return t("skills.library.deploymentStatus.archived");
    if (!control.compatible) return t("skills.batch.incompatible");
    const deployment = control.deployment;
    if (!deployment) return t("skills.library.deploymentStatus.not_deployed");
    if (
      [
        "in_sync",
        "not_deployed",
        "blocked",
        "archived",
        "unsupported",
      ].includes(deployment.status)
    )
      return t(`skills.library.deploymentStatus.${deployment.status}`);
    if (deployment.observed.state === "missing")
      return t("skills.workspaceCard.missingLink");
    return t(`skills.library.observedState.${deployment.observed.state}`);
  };
  const guidance = (control: WorkspaceDeploymentControl) => {
    const status = control.deployment?.status;
    const observed = control.deployment?.observed.state;
    let key = "inspect";
    if (control.workspaceLifecycle === "unavailable") key = "unavailable";
    else if (control.workspaceLifecycle === "archived" || status === "archived")
      key = "archived";
    else if (status === "unsupported") key = "unsupported";
    else if (!control.compatible) key = "incompatible";
    else if (observed === "occupied_directory" || observed === "occupied_file")
      key = "occupied";
    else if (observed === "missing" && status === "drift") key = "missing";
    else if (
      ["redirected_link", "broken_link", "invalid_link"].includes(
        observed ?? "",
      )
    )
      key = "foreign";
    return t(`skills.workspaceCard.guidance.${key}`);
  };
  const openDetails = (event: MouseEvent<HTMLButtonElement>) => {
    opener.current = event.currentTarget;
    setDetailsOpen(true);
  };

  return (
    <>
      <article
        ref={card}
        tabIndex={-1}
        aria-label={skill.displayName}
        className="glass-card skill-surface-card flex min-h-48 min-w-0 flex-col gap-3 rounded-xl border p-4"
      >
        <div className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-3">
          <div className="min-w-0">
            <h3
              className="line-clamp-2 break-words text-sm font-semibold"
              title={skill.displayName}
            >
              {skill.displayName}
            </h3>
            {skill.directory !== skill.displayName && (
              <p
                className="mt-1 truncate font-mono text-[11px] text-muted-foreground"
                title={skill.directory}
              >
                {skill.directory}
              </p>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {controls.map((control) => (
              <div
                key={control.target.consumer}
                className="relative shrink-0"
                data-testid={`${control.target.workspace}-deployment-${skill.id}-${control.target.consumer}`}
              >
                <DeploymentResolutionActions
                  {...control}
                  iconToggle
                  toggleOnly
                />
                {hasIssue(control) && (
                  <AlertTriangle
                    aria-label={`${consumerLabel(control)}: ${reason(control)}`}
                    className="pointer-events-none absolute -right-1 -top-1 h-3.5 w-3.5 rounded-sm bg-background text-amber-600 dark:text-amber-400"
                  />
                )}
              </div>
            ))}
          </div>
        </div>
        {sourceSummary(skill) && (
          <p
            className="truncate text-xs text-muted-foreground"
            title={sourceSummary(skill)}
          >
            {sourceSummary(skill)}
          </p>
        )}
        {skill.description && (
          <p className="line-clamp-3 break-words text-sm leading-relaxed text-muted-foreground/90">
            {skill.description}
          </p>
        )}
        {issues.length > 0 && (
          <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-border/50 pt-2">
            <p
              className="flex min-w-0 flex-1 items-center gap-1.5 text-xs text-amber-700 dark:text-amber-400"
              role="status"
            >
              <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
              <span className="line-clamp-2 break-words">
                {issues.length === 1
                  ? `${consumerLabel(issues[0])}: ${reason(issues[0])}`
                  : t("skills.workspaceCard.issueCount", {
                      count: issues.length,
                    })}
              </span>
            </p>
            <Button
              variant="ghost"
              size="sm"
              className="shrink-0"
              onClick={openDetails}
            >
              {t("skills.workspaceCard.resolve")}
            </Button>
          </div>
        )}
      </article>
      <Dialog
        open={detailsOpen}
        onOpenChange={(open) => {
          if (!busy) setDetailsOpen(open);
        }}
      >
        <SkillsDialogContent
          className="max-w-xl"
          closeBlocked={busy}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            (opener.current?.isConnected
              ? opener.current
              : card.current
            )?.focus();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("skills.workspaceCard.resolve")}</DialogTitle>
            <DialogDescription className="break-words">
              {skill.displayName}
            </DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-6">
            {issues.length === 0 && (
              <div
                className="flex flex-col items-center gap-3 py-6 text-center"
                role="status"
              >
                <CheckCircle2 className="h-8 w-8 text-emerald-600 dark:text-emerald-400" />
                <p className="text-sm font-medium">
                  {t("skills.workspaceCard.resolved")}
                </p>
              </div>
            )}
            {issues.map((control) => (
              <section
                key={control.target.consumer}
                aria-label={consumerLabel(control)}
                className="space-y-4 border-b border-border/60 pb-6 last:border-0 last:pb-0"
              >
                <div className="flex items-start gap-3">
                  <div
                    className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border bg-muted/30"
                    aria-hidden="true"
                  >
                    {control.target.consumer === "claude" ? (
                      <ClaudeIcon size={20} />
                    ) : (
                      <CodexIcon size={20} />
                    )}
                  </div>
                  <div className="min-w-0 space-y-1">
                    <h4 className="text-xs font-medium text-muted-foreground">
                      {consumerLabel(control)}
                    </h4>
                    <p className="text-sm font-semibold">{reason(control)}</p>
                  </div>
                </div>
                <p className="text-sm leading-relaxed text-muted-foreground">
                  {guidance(control)}
                </p>
                {!control.compatible &&
                  skill.compatibility[control.target.consumer].issues.length >
                    0 && (
                    <p className="text-sm leading-relaxed">
                      {skill.compatibility[control.target.consumer].issues.join(
                        "; ",
                      )}
                    </p>
                  )}
                <div className="flex flex-wrap gap-2 empty:hidden">
                  <DeploymentResolutionActions {...control} resolutionOnly />
                </div>
                {control.deployment && (
                  <SkillTechnicalDetails>
                    <dl className="space-y-2">
                      <div>
                        <dt>{t("skills.library.targetPath")}</dt>
                        <dd className="break-all">
                          {control.deployment.observed.targetPath}
                        </dd>
                      </div>
                      <div>
                        <dt>{t("skills.library.expectedTarget")}</dt>
                        <dd className="break-all">
                          {control.deployment.observed.expectedTarget}
                        </dd>
                      </div>
                      {control.deployment.observed.actualTarget && (
                        <div>
                          <dt>{t("skills.library.actualTarget")}</dt>
                          <dd className="break-all">
                            {control.deployment.observed.actualTarget}
                          </dd>
                        </div>
                      )}
                    </dl>
                  </SkillTechnicalDetails>
                )}
              </section>
            ))}
          </DialogBody>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setDetailsOpen(false)}
            >
              {t("common.close")}
            </Button>
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>
    </>
  );
}
