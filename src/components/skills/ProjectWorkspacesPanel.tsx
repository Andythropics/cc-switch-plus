import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Archive,
  ArchiveRestore,
  Loader2,
  MapPin,
  Pencil,
  FolderOpen,
  Layers,
  Unlink,
  Trash2,
  Wrench,
} from "lucide-react";
import { toast } from "sonner";

import {
  WorkspaceSkillCard,
  type WorkspaceDeploymentControl,
  workspaceSkillGridClassName,
} from "@/components/skills/WorkspaceSkillCard";
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
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  useArchiveProjectWorkspace,
  useApplySkillDeployments,
  useDeploymentRecovery,
  useForgetProjectWorkspace,
  useLibrarySkills,
  useProjectWorkspaces,
  useProjectWorkspaceInspectionSession,
  useRegisterProjectWorkspace,
  useRelocateProjectWorkspace,
  useRenameProjectWorkspace,
  useRestoreProjectWorkspace,
  useSkillDeployments,
} from "@/hooks/useSkills";
import {
  DeploymentRecoveryPanel,
  isActionableDeploymentRecoveryFinding,
} from "@/components/skills/DeploymentRecoveryPanel";
import { ProjectSkillImportPanel } from "@/components/skills/ProjectSkillImportPanel";
import { BatchDeploymentDialog } from "@/components/skills/BatchDeploymentDialog";
import {
  showSkillErrorToast,
  skillDiagnosticToastOptions,
} from "@/components/skills/SkillTechnicalDetails";
import {
  ProgressiveSkillListFooter,
  useProgressiveSkillList,
} from "@/components/skills/ProgressiveSkillList";
import { settingsApi } from "@/lib/api/settings";
import {
  deploymentOutcomeLabelKeys,
  successfulDeploymentOutcomes,
} from "@/lib/api/skills";
import type {
  DeploymentBatch,
  DeploymentConsumer,
  DeploymentIntent,
  LibrarySkill,
} from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

interface ProjectWorkspacesPanelProps {
  /** Stable Workspace identity supplied by Activity deep links. */
  focusWorkspaceId?: string | null;
  onInteractionBlockedChange?: (blocked: boolean) => void;
}

export interface ProjectWorkspacesPanelHandle {
  refresh: () => Promise<void>;
  registerProject: () => Promise<void>;
}

function ProjectWorkspaceDeployments({
  workspace,
  projects,
  inspectionSessionId,
  onBusyChange,
}: {
  workspace: ProjectWorkspace;
  projects: ProjectWorkspace[];
  inspectionSessionId: number;
  onBusyChange?: (workspaceId: string, busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const libraryQuery = useLibrarySkills();
  const { data: skills = [] } = libraryQuery;
  const refetchLibrary =
    libraryQuery.refetch ?? (async () => ({ data: skills }));
  const {
    data: claudeState,
    isError: claudeError,
    isFetching: claudeFetching,
  } = useSkillDeployments(
    {
      consumer: "claude",
      workspace: "project",
      workspaceId: workspace.id,
    },
    inspectionSessionId,
  );
  const {
    data: codexState,
    isError: codexError,
    isFetching: codexFetching,
  } = useSkillDeployments(
    {
      consumer: "codex",
      workspace: "project",
      workspaceId: workspace.id,
    },
    inspectionSessionId,
  );
  const apply = useApplySkillDeployments();
  const [batchDialogOpen, setBatchDialogOpen] = useState(false);
  const [batchAction, setBatchAction] = useState<"deploy" | "undeploy">(
    "deploy",
  );
  const [pendingDeployments, setPendingDeployments] = useState(
    new Set<string>(),
  );
  const [batchPending, setBatchPending] = useState(false);
  const deploymentError = claudeError || codexError;
  const deploymentFetching = claudeFetching || codexFetching;
  const deploymentBusy = batchDialogOpen;
  const deployedSkillIds = new Set(
    [...(claudeState?.items ?? []), ...(codexState?.items ?? [])]
      .filter((item) => item.desired)
      .map((item) => item.librarySkillId),
  );
  const deployedSkills = skills.filter((skill) =>
    deployedSkillIds.has(skill.id),
  );
  const progressiveSkills = useProgressiveSkillList(
    deployedSkills,
    `${workspace.id}:${deployedSkills.length}`,
  );

  useEffect(() => {
    onBusyChange?.(workspace.id, deploymentBusy);
    return () => onBusyChange?.(workspace.id, false);
  }, [deploymentBusy, onBusyChange, workspace.id]);

  const applyDeployment = async (intent: DeploymentIntent) => {
    const pendingKey = `${intent.librarySkillId}:${intent.target.consumer}`;
    setPendingDeployments((current) => new Set(current).add(pendingKey));
    try {
      const result = await apply.mutateAsync({ intents: [intent] });
      const item = result.items[0];
      if (!item) throw new Error(t("skills.projects.deploymentFailed"));
      const label = t(deploymentOutcomeLabelKeys[item.outcome]);
      if (!successfulDeploymentOutcomes.has(item.outcome)) {
        toast.error(label, skillDiagnosticToastOptions(item.message));
        return;
      }
      if (item.message) {
        toast.success(label, skillDiagnosticToastOptions(item.message));
        return;
      }
      toast.success(
        t(
          intent.action === "deploy"
            ? "skills.projects.deploySuccess"
            : intent.action === "undeploy"
              ? "skills.projects.undeploySuccess"
              : intent.action === "repair"
                ? "skills.library.repairSuccess"
                : intent.action === "replaceForeignLink"
                  ? "skills.library.replaceForeignLinkSuccess"
                  : "skills.library.forgetSuccess",
        ),
      );
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.deploymentFailed", error);
    } finally {
      setPendingDeployments((current) => {
        const next = new Set(current);
        next.delete(pendingKey);
        return next;
      });
    }
  };

  const applyBatch = async (batch: DeploymentBatch) => {
    setBatchPending(true);
    try {
      const result = await apply.mutateAsync(batch);
      await refetchLibrary();
      return result;
    } finally {
      setBatchPending(false);
    }
  };

  const getDeploymentControl = (
    skill: LibrarySkill,
    consumer: DeploymentConsumer,
  ): WorkspaceDeploymentControl => {
    const state = consumer === "claude" ? claudeState : codexState;
    return {
      skill,
      target: { consumer, workspace: "project", workspaceId: workspace.id },
      deployment: state?.items.find((item) => item.librarySkillId === skill.id),
      compatible: skill.compatibility[consumer].compatible,
      workspaceLifecycle: workspace.lifecycle,
      isPending: pendingDeployments.has(`${skill.id}:${consumer}`),
      deployLabel: t(
        consumer === "claude"
          ? "skills.library.deployClaude"
          : "skills.library.deployCodex",
      ),
      undeployLabel: t(
        consumer === "claude"
          ? "skills.library.undeployClaude"
          : "skills.library.undeployCodex",
      ),
      onApply: applyDeployment,
    };
  };

  return (
    <div className="space-y-3 border-t border-border/50 pt-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">
          {t("skills.projects.deployments")}
        </h3>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={workspace.lifecycle !== "active" || skills.length === 0}
            onClick={() => {
              setBatchAction("deploy");
              setBatchDialogOpen(true);
            }}
          >
            <Layers className="h-4 w-4" />
            {t("skills.projects.addSkills")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={
              !["active", "archived"].includes(workspace.lifecycle) ||
              deployedSkills.length === 0
            }
            onClick={() => {
              setBatchAction("undeploy");
              setBatchDialogOpen(true);
            }}
          >
            <Unlink className="h-4 w-4" />
            {t("skills.batch.undeploy")}
          </Button>
        </div>
      </div>
      {deploymentError && (
        <p
          role="alert"
          className="rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        >
          {t("skills.projects.deploymentLoadError")}
        </p>
      )}
      {skills.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.projects.noLibrarySkills")}
        </p>
      ) : deployedSkills.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.projects.noDeployedSkills")}
        </p>
      ) : (
        <div className={workspaceSkillGridClassName}>
          {progressiveSkills.visibleItems.map((skill) => (
            <WorkspaceSkillCard
              key={skill.id}
              skill={skill}
              controls={[
                getDeploymentControl(skill, "claude"),
                getDeploymentControl(skill, "codex"),
              ]}
            />
          ))}
        </div>
      )}
      <ProgressiveSkillListFooter
        visibleCount={progressiveSkills.visibleCount}
        totalCount={progressiveSkills.totalCount}
        hasMore={progressiveSkills.hasMore}
        onShowMore={progressiveSkills.showMore}
      />
      <BatchDeploymentDialog
        open={batchDialogOpen}
        onOpenChange={setBatchDialogOpen}
        skills={skills}
        projects={projects}
        defaultTarget={{ workspace: "project", workspaceId: workspace.id }}
        targetLocked
        defaultAction={batchAction}
        inspections={[
          ...(claudeState?.items ?? []),
          ...(codexState?.items ?? []),
        ]}
        isPending={batchPending || deploymentFetching}
        onApply={applyBatch}
      />
    </div>
  );
}

export const ProjectWorkspacesPanel = forwardRef<
  ProjectWorkspacesPanelHandle,
  ProjectWorkspacesPanelProps
>(function ProjectWorkspacesPanel(
  { focusWorkspaceId, onInteractionBlockedChange },
  ref,
) {
  const { t } = useTranslation();
  const projectQuery = useProjectWorkspaces();
  const projectInspectionSession = useProjectWorkspaceInspectionSession();
  const projectRecoveryQuery = useDeploymentRecovery({
    workspace: "project",
  });
  const workspaces = projectQuery.data ?? [];
  const isLoading = projectQuery.isLoading;
  const refetchWorkspaces =
    projectQuery.refetch ?? (async () => ({ data: workspaces }));
  const libraryQuery = useLibrarySkills();
  const refetchLibrary =
    libraryQuery.refetch ?? (async () => ({ data: libraryQuery.data ?? [] }));
  const register = useRegisterProjectWorkspace();
  const rename = useRenameProjectWorkspace();
  const archive = useArchiveProjectWorkspace();
  const restore = useRestoreProjectWorkspace();
  const relocate = useRelocateProjectWorkspace();
  const forget = useForgetProjectWorkspace();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const focusedWorkspaceRequest = useRef<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [query, setQuery] = useState("");
  const [renameTarget, setRenameTarget] = useState<ProjectWorkspace | null>(
    null,
  );
  const [renameName, setRenameName] = useState("");
  const [archiveTarget, setArchiveTarget] = useState<ProjectWorkspace | null>(
    null,
  );
  const [relocateTarget, setRelocateTarget] = useState<ProjectWorkspace | null>(
    null,
  );
  const [relocatePath, setRelocatePath] = useState("");
  const [forgetTarget, setForgetTarget] = useState<ProjectWorkspace | null>(
    null,
  );
  const [recoveryWorkspaceId, setRecoveryWorkspaceId] = useState<string | null>(
    null,
  );
  const [projectRecoveryBusy, setProjectRecoveryBusy] = useState(false);
  const [childDeploymentBusyIds, setChildDeploymentBusyIds] = useState<
    Set<string>
  >(new Set());

  const actionableRecoveryWorkspaceIds = new Set(
    (projectRecoveryQuery.data?.findings ?? [])
      .filter(
        (finding) =>
          finding.target.workspace === "project" &&
          Boolean(finding.target.workspaceId) &&
          isActionableDeploymentRecoveryFinding(finding),
      )
      .map((finding) => finding.target.workspaceId!),
  );

  const filteredWorkspaces = workspaces.filter((workspace) => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return true;
    return [workspace.displayName, workspace.id, workspace.rootPath].some(
      (value) => value.toLocaleLowerCase().includes(needle),
    );
  });
  const activeWorkspaces = filteredWorkspaces.filter(
    (workspace) => workspace.lifecycle !== "archived",
  );
  const archivedWorkspaces = filteredWorkspaces.filter(
    (workspace) => workspace.lifecycle === "archived",
  );

  useEffect(() => {
    if (!focusWorkspaceId) {
      focusedWorkspaceRequest.current = null;
      return;
    }
    if (focusedWorkspaceRequest.current === focusWorkspaceId) return;
    const target = workspaces.find(
      (workspace) => workspace.id === focusWorkspaceId,
    );
    if (!target) return;
    focusedWorkspaceRequest.current = focusWorkspaceId;
    setSelectedId(target.id);
    if (target.lifecycle === "archived") setShowArchived(true);
  }, [focusWorkspaceId, workspaces]);
  const lifecycleBusy =
    register.isPending ||
    rename.isPending ||
    archive.isPending ||
    restore.isPending ||
    relocate.isPending ||
    forget.isPending;
  const managementBusy =
    lifecycleBusy || projectRecoveryBusy || childDeploymentBusyIds.size > 0;

  const onChildBusyChange = useCallback(
    (workspaceId: string, busy: boolean) => {
      setChildDeploymentBusyIds((current) => {
        const next = new Set(current);
        if (busy) next.add(workspaceId);
        else next.delete(workspaceId);
        return next;
      });
    },
    [],
  );

  useEffect(() => {
    onInteractionBlockedChange?.(managementBusy);
  }, [managementBusy, onInteractionBlockedChange]);

  useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
    },
    [onInteractionBlockedChange],
  );

  const selected =
    workspaces.find((workspace) => workspace.id === selectedId) ??
    activeWorkspaces[0] ??
    (showArchived ? archivedWorkspaces[0] : undefined);

  const refreshAll = async () => {
    await Promise.all([
      refetchWorkspaces(),
      refetchLibrary(),
      projectInspectionSession.refresh(),
    ]);
  };

  const registerDirectory = async () => {
    const path = await settingsApi.pickDirectory();
    if (!path) return;
    try {
      const result = await register.mutateAsync({ path });
      setSelectedId(result.workspace.id);
      toast.success(t("skills.projects.registerSuccess"));
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.actionFailed", error);
    }
  };

  const openRename = (workspace: ProjectWorkspace) => {
    setRenameTarget(workspace);
    setRenameName(workspace.displayName);
  };

  const submitRename = async () => {
    if (!renameTarget || !renameName.trim()) return;
    try {
      await rename.mutateAsync({
        workspaceId: renameTarget.id,
        displayName: renameName.trim(),
      });
      setRenameTarget(null);
      toast.success(t("skills.projects.renameSuccess"));
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.actionFailed", error);
    }
  };

  const submitArchive = async () => {
    if (!archiveTarget) return;
    try {
      await archive.mutateAsync(archiveTarget.id);
      setArchiveTarget(null);
      toast.success(t("skills.projects.archiveSuccess"));
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.actionFailed", error);
    }
  };

  const restoreWorkspace = async (workspace: ProjectWorkspace) => {
    try {
      await restore.mutateAsync(workspace.id);
      setSelectedId(workspace.id);
      setShowArchived(false);
      toast.success(t("skills.projects.restoreSuccess"));
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.actionFailed", error);
    }
  };

  const openRelocate = async (workspace: ProjectWorkspace) => {
    if (workspace.lifecycle !== "unavailable") return;
    const path = await settingsApi.pickDirectory();
    if (!path) return;
    setRelocateTarget(workspace);
    setRelocatePath(path);
  };

  const submitRelocate = async () => {
    if (!relocateTarget || !relocatePath) return;
    try {
      const result = await relocate.mutateAsync({
        workspaceId: relocateTarget.id,
        path: relocatePath,
      });
      setRelocateTarget(null);
      setSelectedId(result.workspace.id);
      toast.success(
        t(
          result.outcome === "registered_distinct"
            ? "skills.projects.relocateDistinctSuccess"
            : "skills.projects.relocateSuccess",
        ),
      );
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.actionFailed", error);
    }
  };

  const submitForget = async () => {
    if (!forgetTarget) return;
    try {
      await forget.mutateAsync(forgetTarget.id);
      setForgetTarget(null);
      if (selectedId === forgetTarget.id) setSelectedId(null);
      toast.success(t("skills.projects.forgetSuccess"));
    } catch (error) {
      showSkillErrorToast(t, "skills.projects.forgetBlocked", error);
    }
  };

  useImperativeHandle(
    ref,
    () => ({
      refresh: refreshAll,
      registerProject: registerDirectory,
    }),
    [refreshAll, registerDirectory],
  );

  const renderWorkspace = (workspace: ProjectWorkspace) => {
    const selectedWorkspace = selected?.id === workspace.id;
    const undetected = workspace.lifecycle === "unavailable";
    return (
      <article
        key={workspace.id}
        className={`glass-card skill-surface-card space-y-4 rounded-xl border p-4 ${undetected ? `border-destructive/60 ${selectedWorkspace ? "ring-2 ring-destructive/30" : ""}` : selectedWorkspace ? "border-primary ring-2 ring-primary/40" : ""}`}
        data-testid={`project-workspace-${workspace.lifecycle}`}
        data-workspace-id={workspace.id}
      >
        {undetected && (
          <div className="flex items-start gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-destructive">
            <AlertTriangle
              className="mt-0.5 h-5 w-5 shrink-0"
              aria-hidden="true"
            />
            <div className="min-w-0">
              <p className="text-sm font-semibold">
                {t("skills.projects.undetectedTitle")}
              </p>
              <p className="mt-1 text-sm">
                {t("skills.projects.undetectedDescription")}
              </p>
            </div>
          </div>
        )}
        <div className="flex flex-wrap items-start gap-3">
          <button
            className="min-w-0 flex-1 text-left"
            disabled={managementBusy}
            onClick={() => setSelectedId(workspace.id)}
          >
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="min-w-0 break-words font-semibold">
                {workspace.displayName}
              </h3>
              <Badge variant="outline">
                {t(`skills.projects.rootKind.${workspace.rootKind}`)}
              </Badge>
              <Badge
                variant={
                  undetected
                    ? "destructive"
                    : workspace.lifecycle === "active"
                      ? "secondary"
                      : "outline"
                }
              >
                {t(`skills.projects.lifecycle.${workspace.lifecycle}`)}
              </Badge>
            </div>
            <p
              className={`mt-1.5 break-all font-mono text-xs ${undetected ? "text-destructive" : "text-muted-foreground"}`}
            >
              {workspace.rootPath}
            </p>
          </button>
          <div className="ml-auto flex shrink-0 flex-wrap items-center justify-end gap-1">
            {actionableRecoveryWorkspaceIds.has(workspace.id) && (
              <TooltipProvider delayDuration={250}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      data-testid={`project-recovery-trigger-${workspace.id}`}
                      aria-label={t("skills.recovery.open")}
                      disabled={managementBusy}
                      className="text-destructive hover:text-destructive"
                      onClick={() => setRecoveryWorkspaceId(workspace.id)}
                    >
                      <Wrench className="h-4 w-4 text-destructive" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    {t("skills.recovery.navTooltip")}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
            <Button
              variant="ghost"
              size="icon"
              aria-label={t("skills.projects.rename")}
              title={t("skills.projects.rename")}
              disabled={managementBusy}
              onClick={() => openRename(workspace)}
            >
              <Pencil className="h-4 w-4" />
            </Button>
            {workspace.lifecycle !== "archived" && (
              <Button
                variant="outline"
                size="sm"
                disabled={managementBusy}
                onClick={() => setArchiveTarget(workspace)}
              >
                <Archive className="h-4 w-4" />
                {t("skills.projects.archive")}
              </Button>
            )}
            {workspace.lifecycle === "archived" && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={managementBusy}
                  onClick={() => void restoreWorkspace(workspace)}
                >
                  <ArchiveRestore className="h-4 w-4" />
                  {t("skills.projects.restore")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={managementBusy}
                  onClick={() => setForgetTarget(workspace)}
                >
                  <Trash2 className="h-4 w-4" />
                  {t("skills.projects.forget")}
                </Button>
              </>
            )}
            {workspace.lifecycle === "unavailable" && (
              <Button
                variant="outline"
                size="sm"
                disabled={managementBusy}
                onClick={() => void openRelocate(workspace)}
              >
                <MapPin className="h-4 w-4" />
                {t("skills.projects.relocate")}
              </Button>
            )}
          </div>
        </div>
        {selectedWorkspace && (
          <>
            {workspace.lifecycle === "active" && (
              <ProjectSkillImportPanel
                workspaceId={workspace.id}
                inspectionSessionId={
                  projectInspectionSession.inspectionSessionId
                }
              />
            )}
            <ProjectWorkspaceDeployments
              workspace={workspace}
              projects={workspaces}
              inspectionSessionId={projectInspectionSession.inspectionSessionId}
              onBusyChange={onChildBusyChange}
            />
          </>
        )}
      </article>
    );
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {(projectQuery.isError || libraryQuery.isError) && (
        <p
          role="alert"
          className="mx-5 mt-3 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        >
          {t("skills.projects.loadError")}
        </p>
      )}
      <div
        className="flex flex-wrap items-center gap-3 px-5 py-3"
        role="toolbar"
      >
        <ManagementListSearch
          value={query}
          onValueChange={setQuery}
          placeholder={t("skills.searchPlaceholder")}
          ariaLabel={t("skills.searchPlaceholder")}
          clearLabel={t("common.clear")}
          className="mb-0 min-w-0 flex-1"
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-5 pb-5">
        {isLoading ? (
          <div className="flex justify-center py-16">
            <Loader2 className="h-5 w-5 animate-spin" />
          </div>
        ) : filteredWorkspaces.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-16 text-center text-muted-foreground">
            <FolderOpen className="h-10 w-10 opacity-50" />
            <p className="font-medium">
              {workspaces.length === 0
                ? t("skills.projects.empty")
                : t("skills.noResults")}
            </p>
            <p className="max-w-sm text-sm">
              {workspaces.length === 0
                ? t("skills.projects.emptyDescription")
                : t("skills.projects.searchNoResults")}
            </p>
          </div>
        ) : (
          <div className="space-y-4">
            {activeWorkspaces.length === 0 ? (
              <div className="rounded-xl border border-dashed p-6 text-center text-muted-foreground">
                <p className="font-medium">
                  {t("skills.projects.emptyActive")}
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {activeWorkspaces.map(renderWorkspace)}
              </div>
            )}
            {archivedWorkspaces.length > 0 && (
              <section className="space-y-3" data-testid="archived-workspaces">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setShowArchived((visible) => !visible)}
                >
                  <Archive className="h-4 w-4" />
                  {t(
                    showArchived
                      ? "skills.projects.hideArchived"
                      : "skills.projects.showArchived",
                  )}
                </Button>
                {showArchived && (
                  <div className="space-y-3">
                    {archivedWorkspaces.map(renderWorkspace)}
                  </div>
                )}
              </section>
            )}
          </div>
        )}
      </div>

      <Dialog
        open={Boolean(recoveryWorkspaceId)}
        onOpenChange={(open) => {
          if (!open && projectRecoveryBusy) return;
          if (!open) setRecoveryWorkspaceId(null);
        }}
      >
        <SkillsDialogContent
          closeBlocked={projectRecoveryBusy}
          className="max-h-[90vh] max-w-2xl overflow-y-auto p-0"
        >
          <DialogTitle className="sr-only">
            {t("skills.recovery.title")}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("skills.recovery.description")}
          </DialogDescription>
          {recoveryWorkspaceId && (
            <DeploymentRecoveryPanel
              query={{
                workspace: "project",
                workspaceId: recoveryWorkspaceId,
              }}
              onBusyChange={setProjectRecoveryBusy}
            />
          )}
        </SkillsDialogContent>
      </Dialog>

      <Dialog
        open={Boolean(renameTarget)}
        onOpenChange={(open) => {
          if (!open && !rename.isPending) setRenameTarget(null);
        }}
      >
        <SkillsDialogContent
          closeBlocked={
            rename.isPending ||
            Boolean(renameTarget && renameName !== renameTarget.displayName)
          }
        >
          <DialogHeader>
            <DialogTitle>{t("skills.projects.renameTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.renameDescription")}
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4">
            <Label htmlFor="project-workspace-display-name">
              {t("skills.projects.displayName")}
            </Label>
            <Input
              id="project-workspace-display-name"
              value={renameName}
              onChange={(event) => setRenameName(event.target.value)}
              className="mt-2"
            />
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={rename.isPending}
              onClick={() => setRenameTarget(null)}
            >
              {t("skills.projects.cancel")}
            </Button>
            <Button
              onClick={() => void submitRename()}
              disabled={!renameName.trim() || rename.isPending}
            >
              {t("skills.projects.renameConfirm")}
            </Button>
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>

      <Dialog
        open={Boolean(archiveTarget)}
        onOpenChange={(open) => {
          if (!open && !archive.isPending) setArchiveTarget(null);
        }}
      >
        <SkillsDialogContent closeBlocked={archive.isPending}>
          <DialogHeader>
            <DialogTitle>{t("skills.projects.archiveTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.archiveDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={archive.isPending}
              onClick={() => setArchiveTarget(null)}
            >
              {t("skills.projects.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void submitArchive()}
              disabled={archive.isPending}
            >
              {t("skills.projects.archiveConfirm")}
            </Button>
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>

      <Dialog
        open={Boolean(relocateTarget)}
        onOpenChange={(open) => {
          if (!open && !relocate.isPending) setRelocateTarget(null);
        }}
      >
        <SkillsDialogContent closeBlocked={relocate.isPending}>
          <DialogHeader>
            <DialogTitle>{t("skills.projects.relocateTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.relocateDescription")}
            </DialogDescription>
          </DialogHeader>
          <p className="break-all px-6 py-3 font-mono text-xs text-muted-foreground">
            {relocatePath}
          </p>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={relocate.isPending}
              onClick={() => setRelocateTarget(null)}
            >
              {t("skills.projects.cancel")}
            </Button>
            <Button
              onClick={() => void submitRelocate()}
              disabled={relocate.isPending || !relocatePath}
            >
              {t("skills.projects.relocateConfirm")}
            </Button>
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>

      <Dialog
        open={Boolean(forgetTarget)}
        onOpenChange={(open) => {
          if (!open && !forget.isPending) setForgetTarget(null);
        }}
      >
        <SkillsDialogContent closeBlocked={forget.isPending}>
          <DialogHeader>
            <DialogTitle>{t("skills.projects.forgetTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.forgetDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={forget.isPending}
              onClick={() => setForgetTarget(null)}
            >
              {t("skills.projects.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void submitForget()}
              disabled={forget.isPending}
            >
              {t("skills.projects.forgetConfirm")}
            </Button>
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>
    </div>
  );
});

ProjectWorkspacesPanel.displayName = "ProjectWorkspacesPanel";
