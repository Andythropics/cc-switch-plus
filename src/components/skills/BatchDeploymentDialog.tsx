import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Layers, Target } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkillsDialogContent } from "@/components/skills/SkillsDialogContent";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  deploymentOutcomeLabelKeys,
  highVisibilityDeploymentOutcomes,
} from "@/lib/api/skills";
import type {
  DeploymentBatch,
  DeploymentBatchResult,
  DeploymentConsumer,
  DeploymentIntent,
  DeploymentInspection,
  DeploymentTarget,
  LibrarySkill,
} from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";
import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
import {
  getSkillTechnicalDetails,
  SkillTechnicalDetails,
} from "@/components/skills/SkillTechnicalDetails";
import {
  ProgressiveSkillListFooter,
  useProgressiveSkillList,
} from "@/components/skills/ProgressiveSkillList";

export type BatchDeploymentAction = "deploy" | "undeploy";
export type BatchDeploymentTarget = Omit<DeploymentTarget, "consumer">;

export interface BatchDeploymentDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  skills: LibrarySkill[];
  projects?: ProjectWorkspace[];
  defaultTarget?: BatchDeploymentTarget;
  /** Lock target selection when opened from Global or a specific Project. */
  targetLocked?: boolean;
  defaultAction?: BatchDeploymentAction;
  inspections?: DeploymentInspection[];
  isPending?: boolean;
  onTargetChange?: (target: BatchDeploymentTarget) => void;
  onApply: (batch: DeploymentBatch) => Promise<DeploymentBatchResult>;
}

type ConsumerSelection = Record<DeploymentConsumer, boolean>;

const consumers: DeploymentConsumer[] = ["claude", "codex"];
const GLOBAL_BATCH_TARGET: BatchDeploymentTarget = { workspace: "global" };
const EMPTY_INSPECTIONS: DeploymentInspection[] = [];

const targetKey = (target: BatchDeploymentTarget) =>
  target.workspace === "global"
    ? "global"
    : `project:${target.workspaceId ?? ""}`;

const targetFromKey = (
  value: string,
  consumer: DeploymentConsumer,
): DeploymentTarget => {
  if (value === "global") return { consumer, workspace: "global" };
  return {
    consumer,
    workspace: "project",
    workspaceId: value.slice("project:".length),
  };
};

/**
 * Build the consumer selection for a decision point. Deploy starts empty so a
 * single confirmation cannot expose every compatible Skill accidentally;
 * undeploy starts only with recorded desired links for the selected target so
 * an unrelated Library item cannot be unlinked accidentally. The same
 * derivation is used when switching action or target inside the dialog.
 */
const selectionFor = (
  action: BatchDeploymentAction,
  targetValue: string,
  skills: LibrarySkill[],
  inspections: DeploymentInspection[],
) => {
  const targetMatches = (inspection: DeploymentInspection) =>
    inspection.target?.workspace ===
      (targetValue === "global" ? "global" : "project") &&
    (targetValue === "global" ||
      inspection.target?.workspaceId === targetValue.slice("project:".length));

  return Object.fromEntries(
    skills.map((skill) => [
      skill.id,
      Object.fromEntries(
        consumers.map((consumer) => {
          if (action === "deploy") {
            return [consumer, false];
          }
          const desired = inspections.some(
            (inspection) =>
              inspection.librarySkillId === skill.id &&
              inspection.target?.consumer === consumer &&
              targetMatches(inspection) &&
              Boolean(inspection.desired),
          );
          return [consumer, desired];
        }),
      ) as ConsumerSelection,
    ]),
  ) as Record<string, ConsumerSelection>;
};

/**
 * Shared batch deploy/undeploy dialog. It only emits declarative intents; all
 * filesystem work and ordering guarantees remain in the deployment service.
 */
export function BatchDeploymentDialog({
  open,
  onOpenChange,
  skills,
  projects = [],
  defaultTarget = GLOBAL_BATCH_TARGET,
  targetLocked = false,
  defaultAction = "deploy",
  inspections = EMPTY_INSPECTIONS,
  isPending = false,
  onTargetChange,
  onApply,
}: BatchDeploymentDialogProps) {
  const { t } = useTranslation();
  const defaultTargetValue = targetKey(defaultTarget);
  const [action, setAction] = useState<BatchDeploymentAction>(defaultAction);
  const [targetValue, setTargetValue] = useState(defaultTargetValue);
  const [selected, setSelected] = useState<Record<string, ConsumerSelection>>(
    {},
  );
  const [result, setResult] = useState<DeploymentBatchResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selectionTouched = useRef(false);
  const dialogWasOpen = useRef(false);

  useEffect(() => {
    if (!open) {
      dialogWasOpen.current = false;
      selectionTouched.current = false;
      return;
    }
    if (dialogWasOpen.current) return;
    dialogWasOpen.current = true;
    setAction(defaultAction);
    setTargetValue(defaultTargetValue);
    setResult(null);
    setError(null);
    setSelected(
      selectionFor(defaultAction, defaultTargetValue, skills, inspections),
    );
  }, [defaultAction, defaultTargetValue, inspections, open, skills]);

  useEffect(() => {
    // Inspection queries can resolve after the dialog opens. Re-derive the
    // selection while no checkbox or submit decision has been made, but never
    // overwrite an explicit user choice or a returned batch result.
    if (!open || selectionTouched.current) return;
    setSelected(selectionFor(action, targetValue, skills, inspections));
  }, [action, inspections, open, skills, targetValue]);

  const selectedIntents = useMemo(() => {
    const intents: DeploymentIntent[] = [];
    // Keep this deterministic: selected Library order, then Claude/Codex.
    for (const skill of skills) {
      for (const consumer of consumers) {
        if (!selected[skill.id]?.[consumer]) continue;
        const target = targetFromKey(targetValue, consumer);
        intents.push({
          action,
          librarySkillId: skill.id,
          target,
        });
      }
    }
    return intents;
  }, [action, selected, skills, targetValue]);

  const selectedProject = targetValue.startsWith("project:")
    ? projects.find(
        (project) => project.id === targetValue.slice("project:".length),
      )
    : undefined;

  const targetDisabled = (project: ProjectWorkspace) =>
    action === "deploy"
      ? project.lifecycle !== "active"
      : project.lifecycle === "unavailable";

  const selectedTargetBlocked =
    selectedProject !== undefined && targetDisabled(selectedProject);
  const progressiveSkills = useProgressiveSkillList(
    skills,
    `${open}:${action}:${targetValue}`,
  );

  const toggleConsumer = (
    skillId: string,
    consumer: DeploymentConsumer,
    checked: boolean,
  ) => {
    selectionTouched.current = true;
    setSelected((current) => ({
      ...current,
      [skillId]: {
        ...(current[skillId] ?? { claude: false, codex: false }),
        [consumer]: checked,
      },
    }));
    setResult(null);
  };

  const submit = async () => {
    if (selectedIntents.length === 0 || isPending) return;
    // Applying is an explicit user decision. Reconciliation/refetches that
    // arrive while the mutation is pending must not reset the selection or
    // clear the structured result that follows it.
    selectionTouched.current = true;
    setError(null);
    setResult(null);
    try {
      const next = await onApply({ intents: selectedIntents });
      setResult(next);
    } catch (cause) {
      setError(getSkillTechnicalDetails(cause));
    }
  };

  const handleOpenChange = (nextOpen: boolean) => {
    // Radix invokes this for backdrop clicks and Escape as well as the
    // explicit cancel button. Never let those implicit close paths discard an
    // in-flight batch operation.
    if (isPending && !nextOpen) return;
    onOpenChange(nextOpen);
  };

  const targetLabel = (target?: DeploymentTarget) => {
    if (!target) return t("skills.batch.unknownTarget");
    if (target.workspace === "global") return t("skills.batch.global");
    const project = projects.find((item) => item.id === target.workspaceId);
    return project
      ? `${project.displayName} (${project.id})`
      : `${t("skills.batch.project")} (${target.workspaceId ?? ""})`;
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <SkillsDialogContent
        className="max-w-2xl"
        zIndex="alert"
        closeBlocked={isPending}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Layers className="h-4 w-4" />
            {t("skills.batch.title")}
          </DialogTitle>
          <DialogDescription>{t("skills.batch.description")}</DialogDescription>
        </DialogHeader>

        <div className="min-h-0 space-y-4 overflow-auto px-6 py-4">
          <div className="grid gap-3 sm:grid-cols-2">
            <label className="space-y-1 text-sm">
              <span className="font-medium">{t("skills.batch.action")}</span>
              <Select
                value={action}
                onValueChange={(value: BatchDeploymentAction) => {
                  selectionTouched.current = false;
                  setAction(value);
                  setSelected(
                    selectionFor(value, targetValue, skills, inspections),
                  );
                  setResult(null);
                }}
              >
                <SelectTrigger aria-label={t("skills.batch.action")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="deploy">
                    {t("skills.batch.deploy")}
                  </SelectItem>
                  <SelectItem value="undeploy">
                    {t("skills.batch.undeploy")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </label>
            <label className="space-y-1 text-sm">
              <span className="flex items-center gap-1 font-medium">
                <Target className="h-3.5 w-3.5" />
                {t("skills.batch.target")}
              </span>
              <Select
                value={targetValue}
                disabled={targetLocked}
                onValueChange={(value) => {
                  selectionTouched.current = false;
                  setTargetValue(value);
                  setSelected(selectionFor(action, value, skills, inspections));
                  onTargetChange?.(
                    value === "global"
                      ? { workspace: "global" }
                      : {
                          workspace: "project",
                          workspaceId: value.slice("project:".length),
                        },
                  );
                  setResult(null);
                }}
              >
                <SelectTrigger aria-label={t("skills.batch.target")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">
                    {t("skills.batch.global")}
                  </SelectItem>
                  {projects.map((project) => (
                    <SelectItem
                      key={project.id}
                      value={`project:${project.id}`}
                      disabled={targetDisabled(project)}
                    >
                      {project.displayName} ({project.lifecycle})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {selectedProject && (
                <div className="space-y-0.5 text-xs text-muted-foreground">
                  <span className="break-all">
                    {t("skills.batch.workspaceId")}: {selectedProject.id}
                  </span>
                  {selectedTargetBlocked && (
                    <span className="block text-destructive">
                      {t("skills.batch.targetUnavailable")}
                    </span>
                  )}
                </div>
              )}
            </label>
          </div>

          <div className="space-y-2" data-testid="batch-skill-selection">
            {skills.length === 0 ? (
              <p className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
                {t("skills.batch.noSkills")}
              </p>
            ) : (
              progressiveSkills.visibleItems.map((skill) => (
                <div
                  key={skill.id}
                  className="rounded-md border p-3"
                  data-testid={`batch-skill-${skill.id}`}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 break-words font-medium">
                      {skill.displayName}
                    </span>
                    <Badge
                      variant="outline"
                      className="max-w-full whitespace-normal break-all text-left font-mono text-xs"
                    >
                      {skill.directory}
                    </Badge>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-4">
                    {consumers.map((consumer) => {
                      const compatibility = skill.compatibility[consumer];
                      const label =
                        consumer === "claude"
                          ? t("skills.library.consumerClaude")
                          : t("skills.library.consumerCodex");
                      const incompatibleForDeploy =
                        action === "deploy" && !compatibility.compatible;
                      return (
                        <label
                          key={consumer}
                          className="flex items-center gap-2 text-sm"
                        >
                          <Checkbox
                            checked={Boolean(selected[skill.id]?.[consumer])}
                            disabled={incompatibleForDeploy || isPending}
                            onCheckedChange={(value) =>
                              toggleConsumer(skill.id, consumer, value === true)
                            }
                            aria-label={`${skill.displayName} ${label}`}
                          />
                          <span>{label}</span>
                          <Badge
                            variant={
                              compatibility.compatible
                                ? "secondary"
                                : "destructive"
                            }
                          >
                            {compatibility.compatible
                              ? t("skills.batch.compatible")
                              : t("skills.batch.incompatible")}
                          </Badge>
                          {(() => {
                            const inspection = inspections.find(
                              (item) =>
                                item.librarySkillId === skill.id &&
                                item.target?.consumer === consumer &&
                                item.target?.workspace ===
                                  (targetValue === "global"
                                    ? "global"
                                    : "project") &&
                                (targetValue === "global" ||
                                  item.target?.workspaceId ===
                                    targetValue.slice("project:".length)),
                            );
                            return inspection ? (
                              <DeploymentStatusBadge
                                status={inspection.status}
                                observed={inspection.observed}
                              />
                            ) : null;
                          })()}
                        </label>
                      );
                    })}
                  </div>
                </div>
              ))
            )}
            <ProgressiveSkillListFooter
              visibleCount={progressiveSkills.visibleCount}
              totalCount={progressiveSkills.totalCount}
              hasMore={progressiveSkills.hasMore}
              onShowMore={progressiveSkills.showMore}
            />
          </div>

          {error && (
            <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
              <p>{t("skills.batch.applyFailed")}</p>
              <SkillTechnicalDetails details={error} />
            </div>
          )}
          {result && (
            <div
              className="space-y-2 rounded-md border bg-muted/40 p-3 text-sm"
              data-testid="batch-results"
            >
              <p className="font-medium">{t("skills.batch.results")}</p>
              {result.items.length === 0 ? (
                <p className="text-muted-foreground">
                  {t("skills.batch.noResults")}
                </p>
              ) : (
                <ol className="list-decimal space-y-1 pl-5">
                  {result.items.map((item, index) => (
                    <li
                      key={`${item.librarySkillId}-${item.target?.consumer ?? "unknown"}-${item.target?.workspace ?? "unknown"}-${item.target?.workspaceId ?? "global"}-${index}`}
                      data-testid={`batch-result-${index}`}
                      role={
                        item.outcome === "recovery_required"
                          ? "alert"
                          : undefined
                      }
                    >
                      <span className="break-all font-mono text-xs">
                        {item.librarySkillId}
                      </span>{" "}
                      · {item.target?.consumer ?? "?"} ·{" "}
                      {targetLabel(item.target)}:{" "}
                      <Badge
                        variant={
                          highVisibilityDeploymentOutcomes.has(item.outcome)
                            ? "destructive"
                            : "secondary"
                        }
                        data-testid={
                          item.outcome === "recovery_required"
                            ? "batch-recovery-required"
                            : undefined
                        }
                      >
                        {t(deploymentOutcomeLabelKeys[item.outcome])}
                      </Badge>
                      {item.message && (
                        <SkillTechnicalDetails details={item.message} />
                      )}
                    </li>
                  ))}
                </ol>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <span
            className="mr-auto text-sm text-muted-foreground"
            data-testid="batch-selection-count"
            data-count={selectedIntents.length}
            aria-live="polite"
          >
            {t("skills.batch.selectedCount", {
              count: selectedIntents.length,
            })}
          </span>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isPending}
          >
            {t("common.cancel")}
          </Button>
          <Button
            onClick={() => void submit()}
            disabled={
              selectedIntents.length === 0 || selectedTargetBlocked || isPending
            }
            data-testid="batch-apply"
          >
            {isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {action === "deploy"
              ? t("skills.batch.deploy")
              : t("skills.batch.undeploy")}
          </Button>
        </DialogFooter>
      </SkillsDialogContent>
    </Dialog>
  );
}
