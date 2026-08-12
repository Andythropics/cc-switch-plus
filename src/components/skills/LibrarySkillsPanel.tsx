import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  CheckCircle2,
  FileArchive,
  Library,
  Loader2,
  Pencil,
  RefreshCw,
  Search,
  Trash2,
  Upload,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";

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
import { skillsApi } from "@/lib/api";
import type {
  ConsumerCompatibility,
  DeploymentBatch,
  DeploymentConsumer,
  DeploymentIntent,
  DeploymentItemResult,
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

interface LibrarySkillsPanelProps {
  onOpenDiscovery: () => void;
  onOpenProjects?: () => void;
  onOpenGlobal?: () => void;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
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
      className="gap-1"
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
      onOpenGlobal,
      onInteractionBlockedChange,
      onNavigationBlockedChange,
    },
    ref,
  ) => {
    const { t } = useTranslation();
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
    const deleteLibrary = useDeleteLibrarySkill();
    const refreshDeployments = useRefreshSkillDeployments();
    const [query, setQuery] = useState("");
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
    const [batchDialogOpen, setBatchDialogOpen] = useState(false);

    const blocked =
      updateMetadata.isPending ||
      acquireZip.isPending ||
      applyDeployments.isPending ||
      checkLibraryUpdate.isPending ||
      applyLibraryUpdate.isPending ||
      inspectLibraryDeletion.isPending ||
      deleteLibrary.isPending ||
      editing !== null ||
      zipCollision !== null ||
      updateConfirmation !== null;
    const navigationBlocked =
      blocked || deletionInspection !== null || batchDialogOpen;

    useEffect(() => {
      onInteractionBlockedChange?.(navigationBlocked);
      onNavigationBlockedChange?.(navigationBlocked);
    }, [
      navigationBlocked,
      onInteractionBlockedChange,
      onNavigationBlockedChange,
    ]);

    useEffect(() => {
      return () => {
        onInteractionBlockedChange?.(false);
        onNavigationBlockedChange?.(false);
      };
    }, [onInteractionBlockedChange, onNavigationBlockedChange]);

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
        toast.error(String(error));
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
          toast.error(message);
        }
      }
    };

    const openAcquireFromZip = async () => {
      const filePath = await skillsApi.openZipFileDialog();
      if (filePath) await acquireZipFile(filePath);
    };

    const checkForLibraryUpdate = async (skill: LibrarySkill) => {
      try {
        const result = await checkLibraryUpdate.mutateAsync(skill.id);
        setUpdateChecks((current) => ({ ...current, [skill.id]: result }));
        setUpdateResult(null);
      } catch {
        toast.error(t("skills.library.update.checkFailed"));
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
      try {
        const inspection = await inspectLibraryDeletion.mutateAsync(skill.id);
        setDeletionResult(null);
        setDeletionInspection({ skill, inspection });
      } catch {
        toast.error(t("skills.library.delete.inspectFailed"));
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
      try {
        const result = await applyDeployments.mutateAsync({
          intents: [intent],
        });
        const item = result.items[0];
        if (!item) {
          throw new Error(t("skills.library.deploymentFailed"));
        }
        const safeOutcomes = new Set([
          "applied",
          "replaced",
          "already_in_sync",
          "removed",
          "already_absent",
          "forgotten",
        ]);
        if (!safeOutcomes.has(item.outcome)) {
          toast.error(
            item.message
              ? `${item.outcome}: ${item.message}`
              : t("skills.library.deploymentOutcomeBlocked", {
                  outcome: item.outcome,
                }),
          );
          return;
        }
        if (item.message) {
          toast.success(`${item.outcome}: ${item.message}`);
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
        toast.error(String(error));
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

    const renderDeploymentControl = (
      skill: LibrarySkill,
      consumer: DeploymentConsumer,
      state: typeof deploymentState,
    ) => {
      const deployment = state?.items.find(
        (item) => item.librarySkillId === skill.id,
      );
      const compatibility = skill.compatibility[consumer];
      const status = deployment?.status ?? "not_deployed";
      const consumerLabel =
        consumer === "claude"
          ? t("skills.library.consumerClaude")
          : t("skills.library.consumerCodex");
      const deployLabel =
        consumer === "claude"
          ? t("skills.library.deployClaude")
          : t("skills.library.deployCodex");
      const undeployLabel =
        consumer === "claude"
          ? t("skills.library.undeployClaude")
          : t("skills.library.undeployCodex");
      const incompatibilityMessage =
        compatibility.issues.join("; ") ||
        t("skills.library.incompatibleConsumer", { consumer: consumerLabel });

      return (
        <div
          key={consumer}
          id={`deployment-control-${skill.id}-${consumer}-global`}
          className="flex min-w-[16rem] flex-1 flex-wrap items-center gap-2"
          data-testid={`deployment-control-${consumer}`}
        >
          <DeploymentStatusBadge
            status={status}
            observed={deployment?.observed}
            desired={Boolean(deployment?.desired)}
            className="flex-1"
          />
          <DeploymentResolutionActions
            skill={skill}
            target={{ consumer, workspace: "global" }}
            deployment={deployment}
            compatible={compatibility.compatible}
            disabled={blocked}
            isPending={applyDeployments.isPending}
            deployLabel={deployLabel}
            undeployLabel={undeployLabel}
            onApply={applyGlobalDeployment}
          />
          {!compatibility.compatible && (
            <span className="basis-full text-xs text-destructive">
              {incompatibilityMessage}
            </span>
          )}
        </div>
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

    return (
      <div className="flex h-full min-h-0 flex-col">
        <div className="flex items-center gap-3 border-b px-5 py-3">
          <Library className="h-5 w-5 text-primary" />
          <div className="min-w-0 flex-1">
            <h2 className="text-base font-semibold">
              {t("skills.library.title")}
            </h2>
            <p className="text-xs text-muted-foreground">
              {t("skills.library.privateDescription")}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={navigationBlocked}
            onClick={onOpenDiscovery}
          >
            {t("skills.discover")}
          </Button>
          {onOpenProjects && (
            <Button
              variant="outline"
              size="sm"
              disabled={navigationBlocked}
              onClick={onOpenProjects}
            >
              {t("skills.projects.title")}
            </Button>
          )}
          {onOpenGlobal && (
            <Button
              variant="outline"
              size="sm"
              disabled={navigationBlocked}
              onClick={onOpenGlobal}
            >
              {t("skills.global.title")}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={blocked || isRefreshing || skills.length === 0}
            onClick={() => setBatchDialogOpen(true)}
          >
            {t("skills.batch.deploy")}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("skills.refresh")}
            title={t("skills.refresh")}
            disabled={blocked || isRefreshing}
            onClick={() => void refreshAll()}
          >
            <RefreshCw
              className={`h-4 w-4${isRefreshing ? " animate-spin" : ""}`}
            />
          </Button>
        </div>

        {isRefreshing && (
          <p role="status" className="px-5 pt-2 text-xs text-muted-foreground">
            {t("skills.refreshing")}
          </p>
        )}

        {(libraryError || projectQuery.isError || deploymentError) && (
          <p
            role="alert"
            className="mx-5 mt-3 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          >
            {t("skills.global.loadError")}
          </p>
        )}

        <div className="px-5 py-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("skills.searchPlaceholder")}
              className="pl-9"
            />
          </div>
        </div>

        <ScrollArea className="min-h-0 flex-1 px-5 pb-5">
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
            <div className="space-y-3">
              {filtered.map((skill) => (
                <article
                  key={skill.id}
                  className="rounded-xl border bg-card p-4 shadow-sm"
                >
                  <div className="flex items-start gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="font-semibold">{skill.displayName}</h3>
                        <Badge variant="outline" className="font-mono text-xs">
                          {skill.directory}
                        </Badge>
                      </div>
                      {skill.description && (
                        <p className="mt-1 text-sm text-muted-foreground">
                          {skill.description}
                        </p>
                      )}
                      <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                        <Badge variant="outline">
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
                        {sourceSummary(skill) && (
                          <span>{sourceSummary(skill)}</span>
                        )}
                        {skill.source.marketplace && (
                          <span>{skill.source.marketplace}</span>
                        )}
                        {skill.source.repoBranch && (
                          <span className="font-mono">
                            @{skill.source.repoBranch}
                          </span>
                        )}
                        {skill.source.skillPath && (
                          <span className="font-mono">
                            {skill.source.skillPath}
                          </span>
                        )}
                        <CompatibilityBadge
                          consumer={t("skills.library.consumerClaude")}
                          result={skill.compatibility.claude}
                        />
                        <CompatibilityBadge
                          consumer={t("skills.library.consumerCodex")}
                          result={skill.compatibility.codex}
                        />
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={t("skills.library.edit")}
                      onClick={() => beginEdit(skill)}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={t("skills.library.delete.action")}
                      title={t("skills.library.delete.action")}
                      disabled={blocked}
                      onClick={() => void inspectLibraryForDeletion(skill)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                  <div className="mt-3 flex flex-wrap items-center gap-2 border-t pt-3">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={blocked}
                      onClick={() => void checkForLibraryUpdate(skill)}
                    >
                      <RefreshCw className="mr-2 h-4 w-4" />
                      {t("skills.library.update.check")}
                    </Button>
                    {updateChecks[skill.id]?.outcome === "update_available" &&
                      updateChecks[skill.id]?.stageToken && (
                        <Button
                          size="sm"
                          disabled={
                            blocked ||
                            updateChecks[skill.id].affectedDeployments.some(
                              (item) => !item.stagedCompatible,
                            )
                          }
                          onClick={() =>
                            beginLibraryUpdate(skill, updateChecks[skill.id])
                          }
                        >
                          <Upload className="mr-2 h-4 w-4" />
                          {t("skills.library.update.apply")}
                        </Button>
                      )}
                    {updateChecks[skill.id] && (
                      <Badge
                        variant={
                          updateChecks[skill.id].outcome === "update_available"
                            ? "default"
                            : updateChecks[skill.id].outcome === "up_to_date"
                              ? "secondary"
                              : "destructive"
                        }
                        data-testid={`library-update-status-${skill.id}`}
                      >
                        {t(updateOutcomeKey(updateChecks[skill.id].outcome))}
                      </Badge>
                    )}
                  </div>
                  {updateChecks[skill.id] && (
                    <div
                      className="mt-2 space-y-2 rounded-md border bg-muted/40 p-3 text-xs"
                      data-testid={`library-update-details-${skill.id}`}
                    >
                      {updateChecks[skill.id].message && (
                        <p>{updateChecks[skill.id].message}</p>
                      )}
                      {updateChecks[skill.id].localModified && (
                        <p className="font-medium text-destructive">
                          {t("skills.library.update.localModified")}
                        </p>
                      )}
                      <div className="flex flex-wrap gap-x-4 gap-y-1 text-muted-foreground">
                        <span>
                          {t("skills.library.update.recordedHash")}:{" "}
                          <code>
                            {updateChecks[skill.id].recordedContentHash}
                          </code>
                        </span>
                        {updateChecks[skill.id].stagedContentHash && (
                          <span>
                            {t("skills.library.update.stagedHash")}:{" "}
                            <code>
                              {updateChecks[skill.id].stagedContentHash}
                            </code>
                          </span>
                        )}
                      </div>
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
                                      className="underline underline-offset-2"
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
                                      className="underline underline-offset-2"
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
                                    <span>
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
                          updateApplyOutcomeKey(updateResult.outcome as never),
                        )}
                      </p>
                      {updateResult.reason && (
                        <p>
                          {t(updateReasonKey(updateResult.reason as never))}
                        </p>
                      )}
                      {updateResult.message && <p>{updateResult.message}</p>}
                      {updateResult.backupPath && (
                        <p>
                          {t("skills.library.update.backupPath")}:{" "}
                          <code>{updateResult.backupPath}</code>
                        </p>
                      )}
                    </div>
                  )}
                  <div className="mt-3 flex flex-wrap gap-2 border-t pt-3">
                    {renderDeploymentControl(skill, "claude", deploymentState)}
                    {renderDeploymentControl(
                      skill,
                      "codex",
                      codexDeploymentState,
                    )}
                  </div>
                </article>
              ))}
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
          open={updateConfirmation !== null}
          onOpenChange={(open) => {
            if (!open) setUpdateConfirmation(null);
          }}
        >
          <DialogContent>
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
                <p>
                  {updateConfirmation.skill.displayName} (
                  <code>{updateConfirmation.skill.directory}</code>)
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
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.library.update.confirmApply")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog
          open={deletionInspection !== null}
          onOpenChange={(open) => {
            if (!open) setDeletionInspection(null);
          }}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t("skills.library.delete.title")}</DialogTitle>
              <DialogDescription>
                {t("skills.library.delete.description")}
              </DialogDescription>
            </DialogHeader>
            {deletionInspection && (
              <div className="space-y-3 py-2 text-sm">
                <p>
                  {deletionInspection.skill.displayName} (
                  <code>{deletionInspection.skill.directory}</code>)
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
                              className="underline underline-offset-2"
                              href={anchor}
                            >
                              {targetName}
                            </a>
                          ) : onOpenProjects ? (
                            <button
                              type="button"
                              className="underline underline-offset-2"
                              disabled={navigationBlocked}
                              onClick={() => {
                                setDeletionInspection(null);
                                onOpenProjects();
                              }}
                            >
                              {targetName}
                            </button>
                          ) : (
                            <span>{targetName}</span>
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
                {deletionInspection.inspection.message && (
                  <p className="text-xs text-muted-foreground">
                    {deletionInspection.inspection.message}
                  </p>
                )}
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
                        <code>{deletionResult.backupPath}</code>
                      </p>
                    )}
                    {deletionResult.message && <p>{deletionResult.message}</p>}
                    {deletionResult.items.length > 0 && (
                      <ul className="mt-2 list-disc space-y-1 pl-4">
                        {deletionResult.items.map((item) => (
                          <li
                            key={`${item.librarySkillId}-${item.target.consumer}-${item.target.workspace}-${item.target.workspaceId ?? "global"}`}
                          >
                            {item.target.consumer} / {item.target.workspace}:{" "}
                            <code>{item.outcome}</code>
                            {item.message ? ` - ${item.message}` : ""}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </div>
            )}
            <DialogFooter>
              <Button
                variant="outline"
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
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.library.delete.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={editing !== null} onOpenChange={() => setEditing(null)}>
          <DialogContent>
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
              <div className="rounded-md bg-muted px-3 py-2 text-xs text-muted-foreground">
                {t("skills.library.directory")}: {editing?.directory}
              </div>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setEditing(null)}>
                {t("common.cancel")}
              </Button>
              <Button
                onClick={submitMetadata}
                disabled={!displayName.trim() || updateMetadata.isPending}
              >
                {updateMetadata.isPending && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t("skills.library.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog
          open={zipCollision !== null}
          onOpenChange={() => setZipCollision(null)}
        >
          <DialogContent>
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
              <Button variant="outline" onClick={() => setZipCollision(null)}>
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
                <FileArchive className="mr-2 h-4 w-4" />
                {t("skills.library.acquire")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    );
  },
);

LibrarySkillsPanel.displayName = "LibrarySkillsPanel";
