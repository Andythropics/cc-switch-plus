import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  CheckCircle2,
  FileArchive,
  FolderOpen,
  Library,
  Layers,
  Link2,
  Loader2,
  Pencil,
  RefreshCw,
  Trash2,
  Upload,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter } from "@/components/ui/card";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import {
  Dialog,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkillsDialogContent } from "@/components/skills/SkillsDialogContent";
import { ExternalSkillUpdatesPanel } from "@/components/skills/ExternalSkillUpdatesPanel";
import { AvailableSkillUpdatesDialog } from "@/components/skills/AvailableSkillUpdatesDialog";
import { LinkSkillSourceDialog } from "@/components/skills/LinkSkillSourceDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Textarea } from "@/components/ui/textarea";
import {
  useAcquireLibrarySkillsFromZip,
  useApplySkillDeployments,
  useApplyLibrarySkillUpdate,
  useCheckLibrarySkillUpdate,
  useDeleteLibrarySkill,
  useInspectLibrarySkillDeletion,
  useLibrarySkills,
  useProjectWorkspaces,
  useRefreshSkillDeployments,
  useSkillDeployments,
  useUpdateLibrarySkillMetadata,
} from "@/hooks/useSkills";
import { settingsApi, skillsApi } from "@/lib/api";
import {
  deploymentOutcomeLabelKeys,
  successfulDeploymentOutcomes,
} from "@/lib/api/skills";
import type {
  ConsumerCompatibility,
  DeploymentBatch,
  DeploymentConsumer,
  DeploymentIntent,
  DeploymentItemResult,
  DeploymentMutationOutcome,
  DeploymentTarget,
  LibrarySkill,
  LibrarySkillDeletionInspection,
  LibrarySkillUpdateCheckResult,
} from "@/lib/api/skills";
import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
import { DeploymentResolutionActions } from "@/components/skills/DeploymentResolutionActions";
import {
  BatchDeploymentDialog,
  type BatchDeploymentTarget,
} from "@/components/skills/BatchDeploymentDialog";
import {
  showSkillErrorToast,
  skillDiagnosticToastOptions,
  SkillTechnicalDetails,
} from "@/components/skills/SkillTechnicalDetails";
import {
  ProgressiveSkillListFooter,
  useProgressiveSkillList,
} from "@/components/skills/ProgressiveSkillList";

function hasLinkedSource(skill: LibrarySkill) {
  return (
    (skill.source.kind === "git" || skill.source.kind === "marketplace") &&
    Boolean(skill.source.repoOwner && skill.source.repoName)
  );
}

interface LibrarySkillsPanelProps {
  onOpenDiscovery: () => void;
  onOpenProjects?: () => void;
  /** Stable Library identity supplied by Activity deep links. */
  focusLibrarySkillId?: string | null;
  onInteractionBlockedChange?: (blocked: boolean) => void;
}

export interface LibrarySkillsPanelHandle {
  openDiscovery: () => void;
  openAcquireFromZip: () => Promise<void>;
  refresh: () => Promise<void>;
}

const sourceSummary = (skill: LibrarySkill) => {
  if (skill.source.repoOwner && skill.source.repoName) {
    return `${skill.source.repoOwner}/${skill.source.repoName}`;
  }
  return skill.source.url;
};

const updateOutcomeKey = (outcome: LibrarySkillUpdateCheckResult["outcome"]) =>
  `skills.library.update.outcome.${outcome}`;

const updateApplyOutcomeKey = (
  outcome:
    | "updated"
    | "up_to_date"
    | "blocked"
    | "stale"
    | "rolled_back"
    | "recovery_required",
) => `skills.library.update.applyOutcome.${outcome}`;

const updateReasonKey = (
  reason:
    | "local_modification_confirmation_required"
    | "compatibility_regression"
    | "not_updatable"
    | "invalid_candidate"
    | "duplicate_content"
    | "missing_stage"
    | "stale_observation"
    | "compensation_failed",
) => `skills.library.update.reason.${reason}`;

const targetAnchor = (skillId: string, consumer: DeploymentConsumer) =>
  `#deployment-control-${skillId}-${consumer}-global`;

function CompatibilityBadge({
  consumer,
  result,
}: {
  consumer: string;
  result: ConsumerCompatibility;
}) {
  const Icon = result.compatible ? CheckCircle2 : XCircle;
  return (
    <Badge
      variant={result.compatible ? "secondary" : "destructive"}
      title={result.issues.join("\n") || undefined}
      className="h-6 shrink-0 gap-1 px-2 py-0 text-[11px]"
    >
      <Icon className="h-3 w-3" />
      {consumer}
    </Badge>
  );
}

export const LibrarySkillsPanel = forwardRef<
  LibrarySkillsPanelHandle,
  LibrarySkillsPanelProps
>(
  (
    {
      onOpenDiscovery,
      onOpenProjects,
      focusLibrarySkillId,
      onInteractionBlockedChange,
    },
    ref,
  ) => {
    const { t } = useTranslation();
    const [externalInteraction, setExternalInteraction] = useState(false);
    const [sourceLinkSkill, setSourceLinkSkill] = useState<LibrarySkill | null>(
      null,
    );
    const libraryQuery = useLibrarySkills();
    const {
      data: skills = [],
      isLoading,
      isError: libraryError,
      isFetching: libraryFetching,
    } = libraryQuery;
    const projectQuery = useProjectWorkspaces();
    const { data: projects = [], isFetching: projectsFetching } = projectQuery;
    const refetchLibrary =
      libraryQuery.refetch ?? (async () => ({ data: skills }));
    const refetchProjects =
      projectQuery.refetch ?? (async () => ({ data: projects }));
    const {
      data: deploymentState,
      isError: claudeDeploymentError,
      isFetching: claudeDeploymentFetching,
    } = useSkillDeployments({
      consumer: "claude",
      workspace: "global",
    });
    const {
      data: codexDeploymentState,
      isError: codexDeploymentError,
      isFetching: codexDeploymentFetching,
    } = useSkillDeployments({
      consumer: "codex",
      workspace: "global",
    });
    const [batchTarget, setBatchTarget] = useState<BatchDeploymentTarget>({
      workspace: "global",
    });
    const batchClaudeDeploymentQuery = useSkillDeployments(
      batchTarget.workspace === "global"
        ? { consumer: "claude", workspace: "global" }
        : {
            consumer: "claude",
            workspace: "project",
            workspaceId: batchTarget.workspaceId,
          },
    );
    const batchCodexDeploymentQuery = useSkillDeployments(
      batchTarget.workspace === "global"
        ? { consumer: "codex", workspace: "global" }
        : {
            consumer: "codex",
            workspace: "project",
            workspaceId: batchTarget.workspaceId,
          },
    );
    const deploymentError =
      claudeDeploymentError ||
      codexDeploymentError ||
      batchClaudeDeploymentQuery.isError ||
      batchCodexDeploymentQuery.isError;
    const isRefreshing =
      libraryFetching ||
      projectsFetching ||
      claudeDeploymentFetching ||
      codexDeploymentFetching ||
      batchClaudeDeploymentQuery.isFetching ||
      batchCodexDeploymentQuery.isFetching;
    const updateMetadata = useUpdateLibrarySkillMetadata();
    const acquireZip = useAcquireLibrarySkillsFromZip();
    const applyDeployments = useApplySkillDeployments();
    const checkLibraryUpdate = useCheckLibrarySkillUpdate();
    const applyLibraryUpdate = useApplyLibrarySkillUpdate();
    const inspectLibraryDeletion = useInspectLibrarySkillDeletion();
    const inspectDeployedProjects = useInspectLibrarySkillDeletion();
    const deleteLibrary = useDeleteLibrarySkill();
    const refreshDeployments = useRefreshSkillDeployments();
    const [query, setQuery] = useState("");
    const [focusedSkillId, setFocusedSkillId] = useState<string | null>(null);
    const focusedSkillRequest = useRef<string | null>(null);
    const [editing, setEditing] = useState<LibrarySkill | null>(null);
    const [displayName, setDisplayName] = useState("");
    const [description, setDescription] = useState("");
    const [zipCollision, setZipCollision] = useState<{
      filePath: string;
      directory: string;
    } | null>(null);
    const [uniqueDirectory, setUniqueDirectory] = useState("");
    const [updateChecks, setUpdateChecks] = useState<
      Record<string, LibrarySkillUpdateCheckResult>
    >({});
    const [updateConfirmation, setUpdateConfirmation] = useState<{
      skill: LibrarySkill;
      check: LibrarySkillUpdateCheckResult;
    } | null>(null);
    const [confirmLocalModifications, setConfirmLocalModifications] =
      useState(false);
    const [updateResult, setUpdateResult] = useState<{
      skillId: string;
      outcome: string;
      reason?: string;
      backupPath?: string;
      message?: string;
    } | null>(null);
    const [deletionInspection, setDeletionInspection] = useState<{
      skill: LibrarySkill;
      inspection: LibrarySkillDeletionInspection;
    } | null>(null);
    const [deletionResult, setDeletionResult] = useState<{
      skillId: string;
      outcome: string;
      backupPath?: string;
      message?: string;
      items: DeploymentItemResult[];
    } | null>(null);
    const [deployedProjectsDialog, setDeployedProjectsDialog] = useState<{
      skill: LibrarySkill;
      inspection: LibrarySkillDeletionInspection | null;
      error: { kind: "load" | "action"; details: string } | null;
      result: {
        target: DeploymentTarget;
        outcome: DeploymentMutationOutcome;
        message?: string;
      } | null;
    } | null>(null);
    const [batchDialogOpen, setBatchDialogOpen] = useState(false);
    const [allChecksProgress, setAllChecksProgress] = useState<{
      done: number;
      total: number;
    } | null>(null);
    const allChecksRunning = useRef(false);
    const [availableUpdatesOpen, setAvailableUpdatesOpen] = useState(false);
    const availableUpdates = skills.flatMap((skill) => {
      const check = updateChecks[skill.id];
      return check?.outcome === "update_available" ? [{ skill, check }] : [];
    });
    const linkedSkills = skills.filter(hasLinkedSource);
    const [pendingGlobalDeployments, setPendingGlobalDeployments] = useState(
      new Set<string>(),
    );
    const [pendingUpdateChecks, setPendingUpdateChecks] = useState(
      new Set<string>(),
    );
    const [pendingDeletionInspections, setPendingDeletionInspections] =
      useState(new Set<string>());
    const [pendingProjectInspections, setPendingProjectInspections] = useState(
      new Set<string>(),
    );
    const editingDirty = Boolean(
      editing &&
        (displayName !== editing.displayName ||
          description !== (editing.description ?? "")),
    );
    const zipCollisionDirty = Boolean(
      zipCollision && uniqueDirectory !== `${zipCollision.directory}-2`,
    );

    const blocked =
      allChecksProgress !== null ||
      externalInteraction ||
      sourceLinkSkill !== null ||
      updateMetadata.isPending ||
      acquireZip.isPending ||
      applyLibraryUpdate.isPending ||
      deleteLibrary.isPending ||
      editing !== null ||
      zipCollision !== null ||
      updateConfirmation !== null;
    const navigationBlocked =
      availableUpdatesOpen ||
      blocked ||
      deletionInspection !== null ||
      deployedProjectsDialog !== null ||
      batchDialogOpen;

    useEffect(() => {
      onInteractionBlockedChange?.(navigationBlocked);
    }, [navigationBlocked, onInteractionBlockedChange]);

    useEffect(() => {
      return () => {
        onInteractionBlockedChange?.(false);
      };
    }, [onInteractionBlockedChange]);

    const beginEdit = (skill: LibrarySkill) => {
      setEditing(skill);
      setDisplayName(skill.displayName);
      setDescription(skill.description ?? "");
    };

    const submitMetadata = async () => {
      if (!editing || !displayName.trim()) return;
      try {
        await updateMetadata.mutateAsync({
          id: editing.id,
          displayName: displayName.trim(),
          description: description.trim() || undefined,
        });
        setEditing(null);
        toast.success(t("skills.library.updateSuccess"));
      } catch (error) {
        showSkillErrorToast(t, "skills.updateFailed", error);
      }
    };

    const acquireZipFile = async (
      filePath: string,
      directoryNames: Record<string, string> = {},
    ) => {
      try {
        const acquired = await acquireZip.mutateAsync({
          filePath,
          directoryNames,
        });
        toast.success(
          t("skills.library.acquireZipSuccess", { count: acquired.length }),
        );
      } catch (error) {
        const message = String(error);
        const collision = message.match(
          /LIBRARY_DIRECTORY_CONFLICT: '([^']+)'/,
        );
        if (collision) {
          setZipCollision({ filePath, directory: collision[1] });
          setUniqueDirectory(`${collision[1]}-2`);
        } else {
          showSkillErrorToast(t, "skills.library.acquireFailed", error);
        }
      }
    };

    const openAcquireFromZip = async () => {
      const filePath = await skillsApi.openZipFileDialog();
      if (filePath) await acquireZipFile(filePath);
    };

    const checkForLibraryUpdate = async (skill: LibrarySkill) => {
      setPendingUpdateChecks((current) => new Set(current).add(skill.id));
      try {
        const result = await checkLibraryUpdate.mutateAsync(skill.id);
        setUpdateChecks((current) => ({ ...current, [skill.id]: result }));
        setUpdateResult(null);
        return result;
      } catch {
        setUpdateChecks((current) => {
          const next = { ...current };
          delete next[skill.id];
          return next;
        });
        toast.error(t("skills.library.update.checkFailed"));
        return null;
      } finally {
        setPendingUpdateChecks((current) => {
          const next = new Set(current);
          next.delete(skill.id);
          return next;
        });
      }
    };

    const beginLibraryUpdate = (
      skill: LibrarySkill,
      check: LibrarySkillUpdateCheckResult,
    ) => {
      if (
        check.outcome !== "update_available" ||
        !check.stageToken ||
        check.affectedDeployments.some((item) => !item.stagedCompatible)
      )
        return;
      setConfirmLocalModifications(!check.localModified);
      setUpdateConfirmation({ skill, check });
    };

    const submitLibraryUpdate = async () => {
      if (!updateConfirmation) return;
      const { skill, check } = updateConfirmation;
      if (!check.stageToken) return;
      if (check.localModified && !confirmLocalModifications) return;

      try {
        const result = await applyLibraryUpdate.mutateAsync({
          librarySkillId: skill.id,
          observationToken: check.observationToken,
          stageToken: check.stageToken,
          confirmLocalModifications,
        });
        setUpdateResult({
          skillId: skill.id,
          outcome: result.outcome,
          reason: result.reason,
          backupPath: result.backupPath,
          message: result.message,
        });
        const retryableBlockedReasons = new Set([
          "local_modification_confirmation_required",
          "compatibility_regression",
          "duplicate_content",
        ]);
        const stageRemainsUsable =
          result.outcome === "blocked" &&
          result.reason !== undefined &&
          retryableBlockedReasons.has(result.reason);
        if (!stageRemainsUsable) {
          setUpdateChecks((current) => {
            const next = { ...current };
            delete next[skill.id];
            return next;
          });
        }
        setUpdateConfirmation(null);
        if (result.outcome === "updated") {
          toast.success(t("skills.library.update.applied"));
        }
      } catch {
        toast.error(t("skills.library.update.applyFailed"));
      }
    };

    const inspectLibraryForDeletion = async (skill: LibrarySkill) => {
      setPendingDeletionInspections((current) =>
        new Set(current).add(skill.id),
      );
      try {
        const inspection = await inspectLibraryDeletion.mutateAsync(skill.id);
        setDeletionResult(null);
        setDeletionInspection({ skill, inspection });
      } catch {
        toast.error(t("skills.library.delete.inspectFailed"));
      } finally {
        setPendingDeletionInspections((current) => {
          const next = new Set(current);
          next.delete(skill.id);
          return next;
        });
      }
    };

    const inspectDeployedProjectsForSkill = async (skill: LibrarySkill) => {
      setPendingProjectInspections((current) => new Set(current).add(skill.id));
      setDeployedProjectsDialog({
        skill,
        inspection: null,
        error: null,
        result: null,
      });
      try {
        const inspection = await inspectDeployedProjects.mutateAsync(skill.id);
        setDeployedProjectsDialog((current) =>
          current?.skill.id === skill.id
            ? { ...current, inspection, error: null }
            : current,
        );
      } catch (error) {
        setDeployedProjectsDialog((current) =>
          current?.skill.id === skill.id
            ? {
                ...current,
                inspection: null,
                error: { kind: "load", details: String(error) },
              }
            : current,
        );
        toast.error(t("skills.library.deployedProjects.loadError"));
      } finally {
        setPendingProjectInspections((current) => {
          const next = new Set(current);
          next.delete(skill.id);
          return next;
        });
      }
    };

    const submitLibraryDeletion = async () => {
      if (!deletionInspection) return;
      try {
        const result = await deleteLibrary.mutateAsync({
          librarySkillId: deletionInspection.skill.id,
          observationToken: deletionInspection.inspection.observationToken,
        });
        setDeletionResult({
          skillId: deletionInspection.skill.id,
          outcome: result.outcome,
          backupPath: result.backupPath,
          message: result.message,
          items: result.items,
        });
        if (result.outcome === "deleted") {
          setDeletionInspection(null);
          toast.success(t("skills.library.delete.success"));
        } else {
          const refreshed = await inspectLibraryDeletion.mutateAsync(
            deletionInspection.skill.id,
          );
          setDeletionInspection((current) =>
            current ? { ...current, inspection: refreshed } : current,
          );
        }
      } catch {
        toast.error(t("skills.library.delete.failed"));
      }
    };

    const applyGlobalDeployment = async (intent: DeploymentIntent) => {
      const pendingKey = `${intent.librarySkillId}:${intent.target.consumer}`;
      setPendingGlobalDeployments((current) =>
        new Set(current).add(pendingKey),
      );
      try {
        const result = await applyDeployments.mutateAsync({
          intents: [intent],
        });
        const item = result.items[0];
        if (!item) {
          throw new Error(t("skills.library.deploymentFailed"));
        }
        const label = t(deploymentOutcomeLabelKeys[item.outcome]);
        if (!successfulDeploymentOutcomes.has(item.outcome)) {
          toast.error(label, skillDiagnosticToastOptions(item.message));
          return;
        }
        if (item.message) {
          toast.success(label, skillDiagnosticToastOptions(item.message));
          return;
        }
        const consumer = intent.target.consumer;
        const action = intent.action;
        const successKey =
          action === "deploy"
            ? consumer === "claude"
              ? "skills.library.deploySuccess"
              : "skills.library.deployCodexSuccess"
            : action === "undeploy"
              ? consumer === "claude"
                ? "skills.library.undeploySuccess"
                : "skills.library.undeployCodexSuccess"
              : action === "repair"
                ? "skills.library.repairSuccess"
                : action === "replaceForeignLink"
                  ? "skills.library.replaceForeignLinkSuccess"
                  : "skills.library.forgetSuccess";
        toast.success(t(successKey));
      } catch (error) {
        showSkillErrorToast(t, "skills.library.deploymentFailed", error);
      } finally {
        setPendingGlobalDeployments((current) => {
          const next = new Set(current);
          next.delete(pendingKey);
          return next;
        });
      }
    };

    const applyProjectDeployment = async (intent: DeploymentIntent) => {
      try {
        const result = await applyDeployments.mutateAsync({
          intents: [intent],
        });
        const item = result.items[0];
        if (!item) {
          throw new Error(t("skills.library.deployedProjects.actionFailed"));
        }
        const label = t(deploymentOutcomeLabelKeys[item.outcome]);
        setDeployedProjectsDialog((current) =>
          current?.skill.id === intent.librarySkillId
            ? {
                ...current,
                result: {
                  target: intent.target,
                  outcome: item.outcome,
                  message: item.message,
                },
              }
            : current,
        );
        if (!successfulDeploymentOutcomes.has(item.outcome)) {
          toast.error(label, skillDiagnosticToastOptions(item.message));
          return;
        }
        if (item.message) {
          toast.success(label, skillDiagnosticToastOptions(item.message));
        } else {
          toast.success(label);
        }

        await Promise.all([
          refreshDeployments(),
          refetchLibrary(),
          refetchProjects(),
        ]);
        const refreshed = await inspectDeployedProjects.mutateAsync(
          intent.librarySkillId,
        );
        setDeployedProjectsDialog((current) =>
          current?.skill.id === intent.librarySkillId
            ? { ...current, inspection: refreshed, error: null }
            : current,
        );
      } catch (error) {
        setDeployedProjectsDialog((current) =>
          current?.skill.id === intent.librarySkillId
            ? {
                ...current,
                error: { kind: "action", details: String(error) },
              }
            : current,
        );
        showSkillErrorToast(
          t,
          "skills.library.deployedProjects.actionFailed",
          error,
        );
      }
    };

    const applyBatch = async (batch: DeploymentBatch) => {
      const result = await applyDeployments.mutateAsync(batch);
      await Promise.all([
        refreshDeployments(),
        refetchLibrary(),
        refetchProjects(),
      ]);
      return result;
    };

    const refreshAll = async () => {
      await Promise.all([
        refreshDeployments(),
        refetchLibrary(),
        refetchProjects(),
      ]);
    };

    const checkAllUpdates = async () => {
      if (
        allChecksRunning.current ||
        blocked ||
        pendingUpdateChecks.size ||
        !linkedSkills.length
      )
        return;
      allChecksRunning.current = true;
      setAllChecksProgress({ done: 0, total: linkedSkills.length });
      let failed = 0;
      let updates = 0;
      try {
        for (const [index, skill] of linkedSkills.entries()) {
          const result = await checkForLibraryUpdate(skill);
          if (result?.outcome === "update_available") updates++;
          else if (result?.outcome !== "up_to_date") failed++;
          setAllChecksProgress({ done: index + 1, total: linkedSkills.length });
        }
        if (failed)
          toast.error(
            t("skills.library.update.checkAllFailed", { count: failed }),
          );
        else if (updates === 0)
          toast.success(t("skills.library.update.allUpToDate"), {
            className: "border-green-500 text-green-700 dark:text-green-400",
          });
      } finally {
        allChecksRunning.current = false;
        setAllChecksProgress(null);
      }
    };

    const renderGlobalDeploymentButton = (
      skill: LibrarySkill,
      consumer: DeploymentConsumer,
      state: typeof deploymentState,
    ) => {
      const deployment = state?.items.find(
        (item) => item.librarySkillId === skill.id,
      );
      const compatibility = skill.compatibility[consumer];
      const isPending = pendingGlobalDeployments.has(`${skill.id}:${consumer}`);
      const hasDesired = Boolean(deployment?.desired);
      const canDeploy =
        !hasDesired &&
        (deployment?.status ?? "not_deployed") === "not_deployed";
      const isSelected =
        deployment?.status === "in_sync" ||
        deployment?.observed.state === "correct_link" ||
        deployment?.observed.state === "unrecorded_link";
      const deployLabel =
        consumer === "claude"
          ? t("skills.library.deployClaude")
          : t("skills.library.deployCodex");
      const deployedLabel =
        consumer === "claude"
          ? t("skills.library.deployedClaude")
          : t("skills.library.deployedCodex");

      return (
        <Button
          key={consumer}
          type="button"
          variant={isSelected ? "default" : "outline"}
          size="sm"
          className="w-full min-w-0"
          aria-pressed={isSelected}
          aria-busy={isPending}
          id={`deployment-control-${skill.id}-${consumer}-global`}
          data-testid={`global-deploy-${skill.id}-${consumer}`}
          disabled={
            blocked ||
            isPending ||
            (isSelected ? !hasDesired : !canDeploy || !compatibility.compatible)
          }
          onClick={() => {
            if (
              isSelected ? !hasDesired : !canDeploy || !compatibility.compatible
            ) {
              return;
            }
            void applyGlobalDeployment({
              action: isSelected ? "undeploy" : "deploy",
              librarySkillId: skill.id,
              target: { consumer, workspace: "global" },
            });
          }}
        >
          {isPending ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : isSelected ? (
            <Check className="h-3.5 w-3.5" />
          ) : (
            <Link2 className="h-3.5 w-3.5" />
          )}
          <span className="truncate">
            {isSelected ? deployedLabel : deployLabel}
          </span>
        </Button>
      );
    };

    useImperativeHandle(ref, () => ({
      openDiscovery: onOpenDiscovery,
      openAcquireFromZip,
      refresh: refreshAll,
    }));

    const filtered = useMemo(() => {
      const needle = query.trim().toLocaleLowerCase();
      if (!needle) return skills;
      return skills.filter((skill) =>
        [
          skill.displayName,
          skill.description,
          skill.directory,
          skill.source.repoOwner,
          skill.source.repoName,
          skill.source.skillPath,
        ].some((value) => value?.toLocaleLowerCase().includes(needle)),
      );
    }, [query, skills]);
    const progressiveSkills = useProgressiveSkillList(filtered, query);
    const deployedProjectTargets =
      deployedProjectsDialog?.inspection?.targets.filter(
        ({ inspection }) => inspection.target.workspace === "project",
      ) ?? [];

    useEffect(() => {
      if (!focusLibrarySkillId) {
        focusedSkillRequest.current = null;
        setFocusedSkillId(null);
        return;
      }
      if (focusedSkillRequest.current === focusLibrarySkillId) return;
      const target = skills.find((skill) => skill.id === focusLibrarySkillId);
      if (!target) {
        focusedSkillRequest.current = focusLibrarySkillId;
        setFocusedSkillId(null);
        return;
      }
      focusedSkillRequest.current = focusLibrarySkillId;
      setFocusedSkillId(target.id);
      setQuery(target.displayName || target.directory);
    }, [focusLibrarySkillId, skills]);

    return (
      <div className="flex h-full min-h-0 flex-col">
        <div
          className="flex flex-wrap items-center gap-3 px-5 py-3"
          role="toolbar"
        >
          <div className="min-w-0 flex-1 basis-full sm:basis-auto">
            <ManagementListSearch
              value={query}
              onValueChange={setQuery}
              placeholder={t("skills.searchPlaceholder")}
              ariaLabel={t("skills.searchPlaceholder")}
              clearLabel={t("common.clear")}
              className="mb-0"
            />
          </div>
          <ExternalSkillUpdatesPanel
            skills={skills}
            disabled={navigationBlocked}
            onInteractionBlockedChange={setExternalInteraction}
          />
          <Button
            variant="outline"
            size="sm"
            className="shrink-0"
            disabled={blocked || skills.length === 0}
            onClick={() => setBatchDialogOpen(true)}
          >
            <Layers className="h-4 w-4" />
            {t("skills.batch.open")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className={`shrink-0 gap-2${availableUpdates.length && !allChecksProgress ? " border-amber-500 text-amber-700 hover:border-amber-600 hover:text-amber-800 dark:text-amber-400 dark:hover:text-amber-300" : ""}`}
            aria-label={t(
              availableUpdates.length && !allChecksProgress
                ? "skills.library.update.availableCount"
                : "skills.library.update.checkAll",
              { count: availableUpdates.length },
            )}
            aria-busy={allChecksProgress !== null}
            title={t(
              availableUpdates.length && !allChecksProgress
                ? "skills.library.update.availableTitle"
                : "skills.library.update.checkAllHint",
            )}
            disabled={
              blocked ||
              pendingUpdateChecks.size > 0 ||
              linkedSkills.length === 0
            }
            onClick={() =>
              availableUpdates.length
                ? setAvailableUpdatesOpen(true)
                : void checkAllUpdates()
            }
          >
            <RefreshCw
              className={`h-4 w-4${allChecksProgress ? " animate-spin" : ""}`}
            />
            {allChecksProgress
              ? t("skills.library.update.checkAllProgress", allChecksProgress)
              : availableUpdates.length
                ? t("skills.library.update.availableCount", {
                    count: availableUpdates.length,
                  })
                : t("skills.library.update.checkAll")}
          </Button>
        </div>

        {(libraryError || projectQuery.isError || deploymentError) && (
          <p
            role="alert"
            className="mx-5 mt-3 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          >
            {t("skills.global.loadError")}
          </p>
        )}

        <AvailableSkillUpdatesDialog
          open={availableUpdatesOpen}
          onOpenChange={setAvailableUpdatesOpen}
          items={availableUpdates}
          onUpdated={(id) =>
            setUpdateChecks((current) => {
              const next = { ...current };
              delete next[id];
              return next;
            })
          }
          onRecheck={checkAllUpdates}
        />

        {sourceLinkSkill && (
          <LinkSkillSourceDialog
            key={sourceLinkSkill.id}
            skill={sourceLinkSkill}
            onClose={() => setSourceLinkSkill(null)}
          />
        )}
        <ScrollArea className="min-h-0 flex-1 px-5 pb-5 pt-3">
          {isLoading ? (
            <div className="flex justify-center py-16">
              <Loader2 className="h-5 w-5 animate-spin" />
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center gap-3 py-16 text-center text-muted-foreground">
              <Library className="h-10 w-10 opacity-50" />
              <p className="font-medium">{t("skills.library.empty")}</p>
              <p className="max-w-sm text-sm">
                {t("skills.library.emptyDescription")}
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
              {progressiveSkills.visibleItems.map((skill) => (
                <Card
                  key={skill.id}
                  role="article"
                  className={`glass-card skill-surface-card group relative flex h-80 min-w-0 flex-col overflow-hidden${focusedSkillId === skill.id ? " ring-2 ring-primary" : ""}`}
                  data-testid={`library-skill-${skill.id}`}
                >
                  <CardContent className="relative z-10 flex min-h-0 flex-1 flex-col overflow-y-auto p-4 pt-4">
                    <div className="flex items-start gap-3">
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                          <h3
                            className="min-w-0 truncate font-semibold"
                            title={skill.displayName}
                          >
                            {skill.displayName}
                          </h3>
                          <Badge
                            variant="outline"
                            className="h-5 min-w-0 max-w-[60%] shrink border-border-default px-2 py-0 text-left font-mono text-[11px]"
                            title={skill.directory}
                          >
                            <span className="truncate">{skill.directory}</span>
                          </Badge>
                        </div>
                        {(sourceSummary(skill) ||
                          skill.source.marketplace ||
                          skill.source.repoBranch ||
                          skill.source.skillPath) && (
                          <div className="mt-1.5 flex min-w-0 items-center gap-2 overflow-hidden text-xs text-muted-foreground">
                            {sourceSummary(skill) && (
                              <button
                                type="button"
                                className="min-w-0 truncate text-left hover:underline"
                                title={sourceSummary(skill)}
                                disabled={
                                  !skill.source.repoOwner ||
                                  !skill.source.repoName
                                }
                                onClick={() => {
                                  if (
                                    !skill.source.repoOwner ||
                                    !skill.source.repoName
                                  )
                                    return;
                                  const root = `https://github.com/${encodeURIComponent(skill.source.repoOwner)}/${encodeURIComponent(skill.source.repoName)}`;
                                  const path = skill.source.skillPath
                                    ?.split("/")
                                    .map(encodeURIComponent)
                                    .join("/");
                                  const url = path
                                    ? `${root}/tree/${encodeURIComponent(skill.source.repoBranch || "HEAD")}/${path}`
                                    : root;
                                  void settingsApi
                                    .openExternal(url)
                                    .catch((error) =>
                                      showSkillErrorToast(
                                        t,
                                        "skills.external.openError",
                                        error,
                                      ),
                                    );
                                }}
                              >
                                {sourceSummary(skill)}
                              </button>
                            )}
                            {skill.source.marketplace && (
                              <span
                                className="min-w-0 truncate"
                                title={skill.source.marketplace}
                              >
                                {skill.source.marketplace}
                              </span>
                            )}
                            {skill.source.repoBranch && (
                              <span
                                className="min-w-0 truncate font-mono"
                                title={`@${skill.source.repoBranch}`}
                              >
                                @{skill.source.repoBranch}
                              </span>
                            )}
                            {skill.source.skillPath && (
                              <span
                                className="min-w-0 truncate font-mono"
                                title={skill.source.skillPath}
                              >
                                {skill.source.skillPath}
                              </span>
                            )}
                          </div>
                        )}
                        {skill.description && (
                          <p
                            className="mt-2 line-clamp-4 text-sm leading-relaxed text-muted-foreground/90"
                            title={skill.description}
                          >
                            {skill.description}
                          </p>
                        )}
                      </div>
                      <div className="ml-auto flex shrink-0 items-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t("skills.library.reveal")}
                          title={t("skills.library.reveal")}
                          onClick={() =>
                            void skillsApi
                              .revealLibrarySkill(skill.id)
                              .catch((error) =>
                                showSkillErrorToast(
                                  t,
                                  "skills.library.revealFailed",
                                  error,
                                ),
                              )
                          }
                        >
                          <FolderOpen className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t("skills.library.edit")}
                          title={t("skills.library.edit")}
                          onClick={() => beginEdit(skill)}
                        >
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t("skills.library.delete.action")}
                          aria-busy={pendingDeletionInspections.has(skill.id)}
                          title={t("skills.library.delete.action")}
                          disabled={
                            blocked || pendingDeletionInspections.has(skill.id)
                          }
                          onClick={() => void inspectLibraryForDeletion(skill)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                    {updateChecks[skill.id] && (
                      <div
                        className="mt-2 space-y-2 rounded-md border bg-muted/40 p-3 text-xs"
                        data-testid={`library-update-details-${skill.id}`}
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <Badge
                            variant={
                              updateChecks[skill.id].outcome ===
                              "update_available"
                                ? "default"
                                : updateChecks[skill.id].outcome ===
                                    "up_to_date"
                                  ? "secondary"
                                  : "destructive"
                            }
                            data-testid={`library-update-status-${skill.id}`}
                          >
                            {t(
                              updateOutcomeKey(updateChecks[skill.id].outcome),
                            )}
                          </Badge>
                          {updateChecks[skill.id].outcome ===
                            "update_available" &&
                            updateChecks[skill.id].stageToken && (
                              <Button
                                size="sm"
                                disabled={
                                  blocked ||
                                  updateChecks[
                                    skill.id
                                  ].affectedDeployments.some(
                                    (item) => !item.stagedCompatible,
                                  )
                                }
                                onClick={() =>
                                  beginLibraryUpdate(
                                    skill,
                                    updateChecks[skill.id],
                                  )
                                }
                              >
                                <Upload className="h-4 w-4" />
                                {t("skills.library.update.apply")}
                              </Button>
                            )}
                        </div>
                        {updateChecks[skill.id].localModified && (
                          <p className="font-medium text-destructive">
                            {t("skills.library.update.localModified")}
                          </p>
                        )}
                        <SkillTechnicalDetails
                          details={updateChecks[skill.id].message}
                        >
                          <div className="flex flex-wrap gap-x-4 gap-y-1">
                            <span>
                              {t("skills.library.update.recordedHash")}:{" "}
                              <code className="break-all">
                                {updateChecks[skill.id].recordedContentHash}
                              </code>
                            </span>
                            {updateChecks[skill.id].stagedContentHash && (
                              <span>
                                {t("skills.library.update.stagedHash")}:{" "}
                                <code className="break-all">
                                  {updateChecks[skill.id].stagedContentHash}
                                </code>
                              </span>
                            )}
                          </div>
                        </SkillTechnicalDetails>
                        {updateChecks[skill.id].affectedDeployments.length >
                          0 && (
                          <div className="space-y-1">
                            <p className="font-medium">
                              {t("skills.library.update.affectedDeployments")}
                            </p>
                            {updateChecks[skill.id].affectedDeployments.map(
                              (affected) => {
                                const { inspection } = affected;
                                const target = inspection.target;
                                const lifecycle =
                                  inspection.status === "archived"
                                    ? t("skills.library.update.archived")
                                    : t("skills.library.update.active");
                                return (
                                  <div
                                    key={`${target.consumer}-${target.workspace}-${target.workspaceId ?? "global"}`}
                                    className="flex flex-wrap items-center gap-2"
                                  >
                                    {target.workspace === "global" ? (
                                      <a
                                        className="min-w-0 break-all underline underline-offset-2"
                                        href={targetAnchor(
                                          inspection.librarySkillId,
                                          target.consumer,
                                        )}
                                      >
                                        {target.consumer} / {target.workspace}
                                      </a>
                                    ) : onOpenProjects ? (
                                      <button
                                        type="button"
                                        className="min-w-0 break-all underline underline-offset-2"
                                        disabled={navigationBlocked}
                                        onClick={() => {
                                          setUpdateConfirmation(null);
                                          setDeletionInspection(null);
                                          onOpenProjects();
                                        }}
                                      >
                                        {target.consumer} / {target.workspace}
                                        {target.workspaceId
                                          ? ` (${target.workspaceId})`
                                          : ""}
                                      </button>
                                    ) : (
                                      <span className="min-w-0 break-all">
                                        {target.consumer} / {target.workspace}
                                        {target.workspaceId
                                          ? ` (${target.workspaceId})`
                                          : ""}
                                      </span>
                                    )}
                                    <Badge variant="outline">{lifecycle}</Badge>
                                    {!affected.stagedCompatible && (
                                      <span className="text-destructive">
                                        {t(
                                          "skills.library.update.compatibilityRegression",
                                        )}
                                      </span>
                                    )}
                                  </div>
                                );
                              },
                            )}
                          </div>
                        )}
                      </div>
                    )}
                    {updateResult?.skillId === skill.id && (
                      <div
                        className={
                          updateResult.outcome === "recovery_required"
                            ? "mt-2 rounded-md border border-destructive bg-destructive/10 p-3 text-xs text-destructive"
                            : "mt-2 rounded-md border bg-muted/40 p-3 text-xs"
                        }
                        data-testid={`library-update-result-${skill.id}`}
                      >
                        <p className="font-medium">
                          {t(
                            updateApplyOutcomeKey(
                              updateResult.outcome as never,
                            ),
                          )}
                        </p>
                        {updateResult.reason && (
                          <p>
                            {t(updateReasonKey(updateResult.reason as never))}
                          </p>
                        )}
                        <SkillTechnicalDetails details={updateResult.message} />
                        {updateResult.backupPath && (
                          <p>
                            {t("skills.library.update.backupPath")}:{" "}
                            <code className="break-all">
                              {updateResult.backupPath}
                            </code>
                          </p>
                        )}
                      </div>
                    )}
                    <div
                      className="mt-auto flex min-w-0 flex-wrap items-center gap-2 pt-3 text-xs text-muted-foreground"
                      data-testid={`library-skill-summary-${skill.id}`}
                    >
                      <div className="flex min-w-0 items-center gap-2 overflow-hidden">
                        <Badge
                          variant="outline"
                          className="h-6 shrink-0 px-2 py-0 text-[11px]"
                        >
                          {t(
                            skill.source.kind === "git"
                              ? "skills.library.sourceGit"
                              : skill.source.kind === "zip"
                                ? "skills.library.sourceZip"
                                : skill.source.kind === "marketplace"
                                  ? "skills.library.sourceMarketplace"
                                  : "skills.library.sourceLocalImport",
                          )}
                        </Badge>
                        <CompatibilityBadge
                          consumer={t("skills.library.consumerClaude")}
                          result={skill.compatibility.claude}
                        />
                        <CompatibilityBadge
                          consumer={t("skills.library.consumerCodex")}
                          result={skill.compatibility.codex}
                        />
                      </div>
                      <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
                        {!hasLinkedSource(skill) && (
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-7 shrink-0 gap-1 px-2 text-[11px]"
                            disabled={blocked}
                            onClick={() => setSourceLinkSkill(skill)}
                          >
                            <Link2 className="h-3.5 w-3.5" />
                            {t("skills.external.manual")}
                          </Button>
                        )}

                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 shrink-0 gap-1 px-2 text-[11px]"
                          aria-busy={pendingUpdateChecks.has(skill.id)}
                          disabled={
                            blocked || pendingUpdateChecks.has(skill.id)
                          }
                          onClick={() => void checkForLibraryUpdate(skill)}
                        >
                          <RefreshCw
                            className={`h-3.5 w-3.5${pendingUpdateChecks.has(skill.id) ? " animate-spin" : ""}`}
                          />
                          {t("skills.library.update.check")}
                        </Button>
                      </div>
                    </div>
                  </CardContent>
                  <CardFooter
                    className="relative z-10 grid grid-cols-1 gap-2 border-t border-border/50 bg-muted/20 p-3 pt-3 sm:grid-cols-2"
                    data-testid={`library-skill-footer-${skill.id}`}
                  >
                    {renderGlobalDeploymentButton(
                      skill,
                      "claude",
                      deploymentState,
                    )}
                    {renderGlobalDeploymentButton(
                      skill,
                      "codex",
                      codexDeploymentState,
                    )}
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="w-full min-w-0 sm:col-span-2"
                      aria-label={t("skills.library.deployedProjects.action")}
                      aria-busy={pendingProjectInspections.has(skill.id)}
                      title={t("skills.library.deployedProjects.action")}
                      disabled={
                        blocked || pendingProjectInspections.has(skill.id)
                      }
                      onClick={() =>
                        void inspectDeployedProjectsForSkill(skill)
                      }
                    >
                      <FolderOpen className="h-4 w-4" />
                      <span className="truncate">
                        {t("skills.library.deployedProjects.action")}
                      </span>
                    </Button>
                  </CardFooter>
                </Card>
              ))}
              <div className="col-span-full">
                <ProgressiveSkillListFooter
                  visibleCount={progressiveSkills.visibleCount}
                  totalCount={progressiveSkills.totalCount}
                  hasMore={progressiveSkills.hasMore}
                  onShowMore={progressiveSkills.showMore}
                />
              </div>
            </div>
          )}
        </ScrollArea>

        <BatchDeploymentDialog
          open={batchDialogOpen}
          onOpenChange={setBatchDialogOpen}
          skills={filtered}
          projects={projects}
          defaultTarget={batchTarget}
          onTargetChange={setBatchTarget}
          inspections={[
            ...(batchClaudeDeploymentQuery.data?.items ?? []),
            ...(batchCodexDeploymentQuery.data?.items ?? []),
          ]}
          isPending={applyDeployments.isPending || isRefreshing}
          onApply={applyBatch}
        />

        <Dialog
          open={deployedProjectsDialog !== null}
          onOpenChange={(open) => {
            if (
              !open &&
              !inspectDeployedProjects.isPending &&
              !applyDeployments.isPending
            ) {
              setDeployedProjectsDialog(null);
            }
          }}
        >
          <SkillsDialogContent
            closeBlocked={
              inspectDeployedProjects.isPending || applyDeployments.isPending
            }
          >
            <DialogHeader>
              <DialogTitle>
                {t("skills.library.deployedProjects.title")}
              </DialogTitle>
              <DialogDescription>
                {t("skills.library.deployedProjects.description")}
              </DialogDescription>
            </DialogHeader>
            {deployedProjectsDialog && (
              <DialogBody className="space-y-3 text-sm">
                <p className="break-words">
                  {deployedProjectsDialog.skill.displayName} (
                  <code className="break-all">
                    {deployedProjectsDialog.skill.directory}
                  </code>
                  )
                </p>
                {inspectDeployedProjects.isPending &&
                  !deployedProjectsDialog.inspection && (
                    <p role="status" className="text-muted-foreground">
                      {t("skills.library.deployedProjects.loading")}
                    </p>
                  )}
                {deployedProjectsDialog.error && (
                  <div
                    role="alert"
                    className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-destructive"
                  >
                    <p>
                      {t(
                        deployedProjectsDialog.error.kind === "load"
                          ? "skills.library.deployedProjects.loadError"
                          : "skills.library.deployedProjects.actionFailed",
                      )}
                    </p>
                    <SkillTechnicalDetails
                      details={deployedProjectsDialog.error.details}
                    />
                  </div>
                )}
                {deployedProjectsDialog.inspection &&
                  deployedProjectsDialog.error?.kind !== "load" &&
                  (deployedProjectTargets.length === 0 ? (
                    <p className="text-muted-foreground">
                      {t("skills.library.deployedProjects.empty")}
                    </p>
                  ) : (
                    <div className="space-y-3">
                      {deployedProjectTargets.map(
                        ({ inspection: deployment }, index) => {
                          const workspace = projects.find(
                            (project) =>
                              project.id === deployment.target.workspaceId,
                          );
                          const workspaceId =
                            deployment.target.workspaceId ?? "";
                          const workspaceName =
                            workspace?.displayName ??
                            t(
                              "skills.library.deployedProjects.unknownWorkspace",
                              {
                                workspaceId,
                              },
                            );
                          const workspaceLifecycle =
                            workspace?.lifecycle ?? "unavailable";
                          const consumerLabel = t(
                            deployment.target.consumer === "claude"
                              ? "skills.library.consumerClaude"
                              : "skills.library.consumerCodex",
                          );
                          return (
                            <div
                              key={`${deployment.target.consumer}-${deployment.target.workspaceId ?? "unknown"}-${index}`}
                              className="space-y-2 rounded-md border p-3"
                              data-testid={`deployed-project-row-${deployment.target.consumer}-${workspaceId}`}
                            >
                              <div className="flex flex-wrap items-start justify-between gap-2">
                                <div className="min-w-0">
                                  <p className="font-medium">{workspaceName}</p>
                                  <p className="break-all text-xs text-muted-foreground">
                                    {workspace?.rootPath ?? workspaceId}
                                  </p>
                                </div>
                                <Badge variant="outline">{consumerLabel}</Badge>
                              </div>
                              <DeploymentStatusBadge
                                status={deployment.status}
                                observed={deployment.observed}
                              />
                              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs text-muted-foreground">
                                <dt>{t("skills.library.targetPath")}</dt>
                                <dd className="break-all font-mono">
                                  {deployment.observed.targetPath}
                                </dd>
                                {deployment.observed.actualTarget && (
                                  <>
                                    <dt>{t("skills.library.actualTarget")}</dt>
                                    <dd className="break-all font-mono">
                                      {deployment.observed.actualTarget}
                                    </dd>
                                  </>
                                )}
                                {deployment.observed.expectedTarget && (
                                  <>
                                    <dt>
                                      {t("skills.library.expectedTarget")}
                                    </dt>
                                    <dd className="break-all font-mono">
                                      {deployment.observed.expectedTarget}
                                    </dd>
                                  </>
                                )}
                              </dl>
                              <div className="flex flex-wrap justify-end gap-2 border-t pt-2">
                                <DeploymentResolutionActions
                                  skill={deployedProjectsDialog.skill}
                                  target={deployment.target}
                                  deployment={deployment}
                                  compatible={
                                    deployedProjectsDialog.skill.compatibility[
                                      deployment.target.consumer
                                    ].compatible
                                  }
                                  workspaceLifecycle={workspaceLifecycle}
                                  disabled={blocked}
                                  isPending={applyDeployments.isPending}
                                  deployLabel={t("skills.projects.deploy")}
                                  undeployLabel={t("skills.projects.undeploy")}
                                  onApply={applyProjectDeployment}
                                />
                              </div>
                            </div>
                          );
                        },
                      )}
                    </div>
                  ))}
                {deployedProjectsDialog.result && (
                  <div
                    className="rounded-md border bg-muted/40 p-3 text-xs"
                    data-testid="deployed-project-action-result"
                  >
                    <p className="font-medium">
                      {t("skills.library.deployedProjects.outcome")}:{" "}
                      {t(
                        deploymentOutcomeLabelKeys[
                          deployedProjectsDialog.result.outcome
                        ],
                      )}
                    </p>
                    <SkillTechnicalDetails
                      details={deployedProjectsDialog.result.message}
                    />
                  </div>
                )}
              </DialogBody>
            )}
            <DialogFooter>
              <Button
                variant="outline"
                disabled={
                  inspectDeployedProjects.isPending ||
                  applyDeployments.isPending
                }
                onClick={() => setDeployedProjectsDialog(null)}
              >
                {t("common.close")}
              </Button>
            </DialogFooter>
          </SkillsDialogContent>
        </Dialog>

        <Dialog
          open={updateConfirmation !== null}
          onOpenChange={(open) => {
            if (!open && !applyLibraryUpdate.isPending) {
              setUpdateConfirmation(null);
            }
          }}
        >
          <SkillsDialogContent
            closeBlocked={
              applyLibraryUpdate.isPending ||
              Boolean(
                updateConfirmation?.check.localModified &&
                  confirmLocalModifications,
              )
            }
          >
            <DialogHeader>
              <DialogTitle>
                {t("skills.library.update.confirmTitle")}
              </DialogTitle>
              <DialogDescription>
                {t("skills.library.update.confirmDescription")}
              </DialogDescription>
            </DialogHeader>
            {updateConfirmation && (
              <div className="space-y-3 py-2 text-sm">
                <p className="break-words">
                  {updateConfirmation.skill.displayName} (
                  <code className="break-all">
                    {updateConfirmation.skill.directory}
                  </code>
                  )
                </p>
                {updateConfirmation.check.localModified && (
                  <label className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-destructive">
                    <input
                      type="checkbox"
                      checked={confirmLocalModifications}
                      onChange={(event) =>
                        setConfirmLocalModifications(event.target.checked)
                      }
                      aria-label={t(
                        "skills.library.update.confirmLocalModifications",
                      )}
                    />
                    <span>
                      {t("skills.library.update.confirmLocalModifications")}
                    </span>
                  </label>
                )}
                <p className="text-xs text-muted-foreground">
                  {t("skills.library.update.backupNotice")}
                </p>
              </div>
            )}
            <DialogFooter>
              <Button
                variant="outline"
                disabled={applyLibraryUpdate.isPending}
                onClick={() => setUpdateConfirmation(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={() => void submitLibraryUpdate()}
                disabled={
                  applyLibraryUpdate.isPending ||
                  (Boolean(updateConfirmation?.check.localModified) &&
                    !confirmLocalModifications)
                }
              >
                {applyLibraryUpdate.isPending && (
                  <Loader2 className="h-4 w-4 animate-spin" />
                )}
                {t("skills.library.update.confirmApply")}
              </Button>
            </DialogFooter>
          </SkillsDialogContent>
        </Dialog>

        <Dialog
          open={deletionInspection !== null}
          onOpenChange={(open) => {
            if (!open && !deleteLibrary.isPending) {
              setDeletionInspection(null);
            }
          }}
        >
          <SkillsDialogContent closeBlocked={deleteLibrary.isPending}>
            <DialogHeader>
              <DialogTitle>{t("skills.library.delete.title")}</DialogTitle>
              <DialogDescription>
                {t("skills.library.delete.description")}
              </DialogDescription>
            </DialogHeader>
            {deletionInspection && (
              <DialogBody className="space-y-3 text-sm">
                <p>
                  {deletionInspection.skill.displayName} (
                  <code className="break-all">
                    {deletionInspection.skill.directory}
                  </code>
                  )
                </p>
                {deletionInspection.inspection.targets.length === 0 ? (
                  <p className="text-muted-foreground">
                    {t("skills.library.delete.noDeployments")}
                  </p>
                ) : (
                  <div className="space-y-2">
                    {deletionInspection.inspection.targets.map((target) => {
                      const deployment = target.inspection;
                      const targetName = `${deployment.target.consumer} / ${deployment.target.workspace}${deployment.target.workspaceId ? ` (${deployment.target.workspaceId})` : ""}`;
                      const anchor =
                        deployment.target.workspace === "global"
                          ? targetAnchor(
                              deployment.librarySkillId,
                              deployment.target.consumer,
                            )
                          : undefined;
                      return (
                        <div
                          key={`${deployment.target.consumer}-${deployment.target.workspace}-${deployment.target.workspaceId ?? "global"}`}
                          className="flex flex-wrap items-center gap-2 rounded-md border p-2 text-xs"
                        >
                          {anchor ? (
                            <a
                              className="min-w-0 break-all underline underline-offset-2"
                              href={anchor}
                            >
                              {targetName}
                            </a>
                          ) : onOpenProjects ? (
                            <button
                              type="button"
                              className="min-w-0 break-all underline underline-offset-2"
                              disabled={navigationBlocked}
                              onClick={() => {
                                setDeletionInspection(null);
                                onOpenProjects();
                              }}
                            >
                              {targetName}
                            </button>
                          ) : (
                            <span className="min-w-0 break-all">
                              {targetName}
                            </span>
                          )}
                          <Badge
                            variant={
                              target.actionRequired === "forget"
                                ? "destructive"
                                : "secondary"
                            }
                          >
                            {t(
                              target.actionRequired === "forget"
                                ? "skills.library.delete.forgetRequired"
                                : "skills.library.delete.removeExpectedLink",
                            )}
                          </Badge>
                        </div>
                      );
                    })}
                  </div>
                )}
                {deletionInspection.inspection.blocked && (
                  <p className="text-xs text-destructive">
                    {t("skills.library.delete.blockedDescription")}
                  </p>
                )}
                <SkillTechnicalDetails
                  details={deletionInspection.inspection.message}
                />
                {deletionResult?.skillId === deletionInspection.skill.id && (
                  <div
                    className={
                      deletionResult.outcome === "recovery_required"
                        ? "rounded-md border border-destructive bg-destructive/10 p-3 text-xs text-destructive"
                        : "rounded-md border bg-muted/40 p-3 text-xs"
                    }
                    data-testid={`library-delete-result-${deletionInspection.skill.id}`}
                  >
                    <p className="font-medium">
                      {t(
                        `skills.library.delete.outcome.${deletionResult.outcome}`,
                      )}
                    </p>
                    {deletionResult.backupPath && (
                      <p>
                        {t("skills.library.update.backupPath")}:{" "}
                        <code className="break-all">
                          {deletionResult.backupPath}
                        </code>
                      </p>
                    )}
                    <SkillTechnicalDetails details={deletionResult.message} />
                    {deletionResult.items.length > 0 && (
                      <ul className="mt-2 list-disc space-y-1 pl-4">
                        {deletionResult.items.map((item) => (
                          <li
                            key={`${item.librarySkillId}-${item.target.consumer}-${item.target.workspace}-${item.target.workspaceId ?? "global"}`}
                          >
                            {item.target.consumer} / {item.target.workspace}:{" "}
                            <code className="break-all">{item.outcome}</code>
                            <SkillTechnicalDetails details={item.message} />
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </DialogBody>
            )}
            <DialogFooter>
              <Button
                variant="outline"
                disabled={deleteLibrary.isPending}
                onClick={() => setDeletionInspection(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="destructive"
                onClick={() => void submitLibraryDeletion()}
                disabled={deleteLibrary.isPending}
              >
                {deleteLibrary.isPending && (
                  <Loader2 className="h-4 w-4 animate-spin" />
                )}
                {t("skills.library.delete.confirm")}
              </Button>
            </DialogFooter>
          </SkillsDialogContent>
        </Dialog>

        <Dialog
          open={editing !== null}
          onOpenChange={(open) => {
            if (!open && !updateMetadata.isPending) setEditing(null);
          }}
        >
          <SkillsDialogContent
            closeBlocked={updateMetadata.isPending || editingDirty}
          >
            <DialogHeader>
              <DialogTitle>{t("skills.library.editTitle")}</DialogTitle>
              <DialogDescription>
                {t("skills.library.editDescription")}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-4 py-2">
              <div className="space-y-2">
                <Label htmlFor="library-display-name">
                  {t("skills.library.displayName")}
                </Label>
                <Input
                  id="library-display-name"
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="library-description">
                  {t("skills.library.description")}
                </Label>
                <Textarea
                  id="library-description"
                  value={description}
                  onChange={(event) => setDescription(event.target.value)}
                />
              </div>
              <div className="break-all rounded-md bg-muted px-3 py-2 text-xs text-muted-foreground">
                {t("skills.library.directory")}: {editing?.directory}
              </div>
            </div>
            <DialogFooter>
              <Button
                variant="outline"
                disabled={updateMetadata.isPending}
                onClick={() => setEditing(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={submitMetadata}
                disabled={!displayName.trim() || updateMetadata.isPending}
              >
                {updateMetadata.isPending && (
                  <Loader2 className="h-4 w-4 animate-spin" />
                )}
                {t("skills.library.save")}
              </Button>
            </DialogFooter>
          </SkillsDialogContent>
        </Dialog>

        <Dialog
          open={zipCollision !== null}
          onOpenChange={(open) => {
            if (!open && !acquireZip.isPending) setZipCollision(null);
          }}
        >
          <SkillsDialogContent
            closeBlocked={acquireZip.isPending || zipCollisionDirty}
          >
            <DialogHeader>
              <DialogTitle>{t("skills.library.collisionTitle")}</DialogTitle>
              <DialogDescription>
                {t("skills.library.collisionDescription", {
                  directory: zipCollision?.directory,
                })}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2 py-2">
              <Label htmlFor="library-unique-directory">
                {t("skills.library.directory")}
              </Label>
              <Input
                id="library-unique-directory"
                value={uniqueDirectory}
                onChange={(event) => setUniqueDirectory(event.target.value)}
              />
            </div>
            <DialogFooter>
              <Button
                variant="outline"
                disabled={acquireZip.isPending}
                onClick={() => setZipCollision(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                disabled={!uniqueDirectory.trim() || acquireZip.isPending}
                onClick={async () => {
                  if (!zipCollision) return;
                  const pending = zipCollision;
                  setZipCollision(null);
                  await acquireZipFile(pending.filePath, {
                    [pending.directory]: uniqueDirectory.trim(),
                  });
                }}
              >
                <FileArchive className="h-4 w-4" />
                {t("skills.library.acquire")}
              </Button>
            </DialogFooter>
          </SkillsDialogContent>
        </Dialog>
      </div>
    );
  },
);

LibrarySkillsPanel.displayName = "LibrarySkillsPanel";
