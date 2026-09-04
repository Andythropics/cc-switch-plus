import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Search } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { BatchDeploymentDialog } from "@/components/skills/BatchDeploymentDialog";
import { DeploymentResolutionActions } from "@/components/skills/DeploymentResolutionActions";
import { GlobalSkillImportPanel } from "@/components/skills/GlobalSkillImportPanel";
import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
import {
  showSkillErrorToast,
  skillDiagnosticToastOptions,
} from "@/components/skills/SkillTechnicalDetails";
import {
  ProgressiveSkillListFooter,
  useProgressiveSkillList,
} from "@/components/skills/ProgressiveSkillList";
import {
  useApplySkillDeployments,
  useLibrarySkills,
  useInspectGlobalSkillImports,
  useProjectWorkspaces,
  useRefreshSkillDeployments,
  useSkillDeployments,
} from "@/hooks/useSkills";
import {
  deploymentOutcomeLabelKeys,
  successfulDeploymentOutcomes,
} from "@/lib/api/skills";
import type {
  DeploymentBatch,
  DeploymentConsumer,
  DeploymentIntent,
  DeploymentStatus,
  LibrarySkill,
} from "@/lib/api/skills";

interface GlobalSkillsPanelProps {
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

export interface GlobalSkillsPanelHandle {
  refresh: () => Promise<void>;
  openBatchUndeploy: () => void;
}

const statuses: DeploymentStatus[] = [
  "not_deployed",
  "in_sync",
  "drift",
  "conflict",
  "orphaned",
  "blocked",
  "archived",
  "unsupported",
];

const sourceSummary = (skill: LibrarySkill) => {
  if (skill.source.repoOwner && skill.source.repoName) {
    return `${skill.source.repoOwner}/${skill.source.repoName}`;
  }
  return skill.source.url;
};

export const GlobalSkillsPanel = forwardRef<
  GlobalSkillsPanelHandle,
  GlobalSkillsPanelProps
>(({ onInteractionBlockedChange, onNavigationBlockedChange }, ref) => {
  const { t } = useTranslation();
  const libraryQuery = useLibrarySkills();
  const {
    data: skills = [],
    isLoading,
    isError: libraryError,
    isFetching: libraryFetching,
  } = libraryQuery;
  const projectQuery = useProjectWorkspaces();
  const {
    data: projects = [],
    isFetching: projectsFetching,
    isError: projectError,
  } = projectQuery;
  const refetchLibrary =
    libraryQuery.refetch ?? (async () => ({ data: skills }));
  const refetchProjects =
    projectQuery.refetch ?? (async () => ({ data: projects }));
  const claudeQuery = useSkillDeployments({
    consumer: "claude",
    workspace: "global",
  });
  const codexQuery = useSkillDeployments({
    consumer: "codex",
    workspace: "global",
  });
  const deploymentLoading = claudeQuery.isLoading || codexQuery.isLoading;
  const apply = useApplySkillDeployments();
  const globalImportsQuery = useInspectGlobalSkillImports();
  const refreshDeployments = useRefreshSkillDeployments();
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<DeploymentStatus | "all">(
    "all",
  );
  const [batchOpen, setBatchOpen] = useState(false);
  const [pendingDeployments, setPendingDeployments] = useState(
    new Set<string>(),
  );
  const [batchPending, setBatchPending] = useState(false);
  const [globalImportBusy, setGlobalImportBusy] = useState(false);

  const allInspections = useMemo(
    () => [
      ...(claudeQuery.data?.items ?? []),
      ...(codexQuery.data?.items ?? []),
    ],
    [claudeQuery.data?.items, codexQuery.data?.items],
  );
  const globallyDeployedSkillIds = useMemo(
    () =>
      new Set(
        allInspections
          .filter((item) => item.target.workspace === "global" && item.desired)
          .map((item) => item.librarySkillId),
      ),
    [allInspections],
  );
  const deploymentError = claudeQuery.isError || codexQuery.isError;
  const isRefreshing =
    libraryFetching ||
    projectsFetching ||
    claudeQuery.isFetching ||
    codexQuery.isFetching ||
    globalImportsQuery.isFetching;
  const navigationBlocked = batchOpen || globalImportBusy;

  useEffect(() => {
    onInteractionBlockedChange?.(navigationBlocked);
    onNavigationBlockedChange?.(navigationBlocked);
  }, [
    navigationBlocked,
    onInteractionBlockedChange,
    onNavigationBlockedChange,
  ]);

  useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
      onNavigationBlockedChange?.(false);
    },
    [onInteractionBlockedChange, onNavigationBlockedChange],
  );

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return skills.filter((skill) => {
      if (!globallyDeployedSkillIds.has(skill.id)) return false;
      const matchesQuery =
        !needle ||
        [
          skill.displayName,
          skill.directory,
          skill.description,
          sourceSummary(skill),
        ].some((value) => value?.toLocaleLowerCase().includes(needle));
      if (!matchesQuery) return false;
      if (statusFilter === "all") return true;
      return ["claude", "codex"].some((consumer) => {
        const inspection = allInspections.find(
          (item) =>
            item.librarySkillId === skill.id &&
            item.target.workspace === "global" &&
            item.target.consumer === consumer,
        );
        return (inspection?.status ?? "not_deployed") === statusFilter;
      });
    });
  }, [globallyDeployedSkillIds, query, skills, statusFilter]);
  const progressiveSkills = useProgressiveSkillList(
    filtered,
    `${query}:${statusFilter}`,
  );

  const applyDeployment = async (intent: DeploymentIntent) => {
    const pendingKey = `${intent.librarySkillId}:${intent.target.consumer}`;
    setPendingDeployments((current) => new Set(current).add(pendingKey));
    try {
      const result = await apply.mutateAsync({ intents: [intent] });
      const item = result.items[0];
      if (!item) throw new Error(t("skills.library.deploymentFailed"));
      const label = t(deploymentOutcomeLabelKeys[item.outcome]);
      if (successfulDeploymentOutcomes.has(item.outcome)) {
        toast.success(label, skillDiagnosticToastOptions(item.message));
      } else {
        toast.error(label, skillDiagnosticToastOptions(item.message));
      }
    } catch (error) {
      showSkillErrorToast(t, "skills.library.deploymentFailed", error);
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
      await Promise.all([
        refreshDeployments(),
        refetchLibrary(),
        refetchProjects(),
        globalImportsQuery.refetch(),
      ]);
      return result;
    } finally {
      setBatchPending(false);
    }
  };

  const refresh = async () => {
    await Promise.all([
      refreshDeployments(),
      refetchLibrary(),
      refetchProjects(),
      globalImportsQuery.refetch(),
    ]);
  };

  useImperativeHandle(ref, () => ({
    refresh,
    openBatchUndeploy: () => setBatchOpen(true),
  }));

  const renderConsumer = (
    skill: LibrarySkill,
    consumer: DeploymentConsumer,
  ) => {
    const state = consumer === "claude" ? claudeQuery.data : codexQuery.data;
    const deployment = state?.items.find(
      (item) => item.librarySkillId === skill.id,
    );
    const compatible = skill.compatibility[consumer];
    const isPending = pendingDeployments.has(`${skill.id}:${consumer}`);
    return (
      <div
        key={consumer}
        className="flex min-w-[15rem] flex-1 flex-wrap items-center gap-2"
        data-testid={`global-deployment-${skill.id}-${consumer}`}
      >
        <span className="text-xs font-medium">
          {consumer === "claude"
            ? t("skills.library.consumerClaude")
            : t("skills.library.consumerCodex")}
        </span>
        <DeploymentStatusBadge
          status={deployment?.status ?? "not_deployed"}
          observed={deployment?.observed}
        />
        <DeploymentResolutionActions
          skill={skill}
          target={{ consumer, workspace: "global" }}
          deployment={deployment}
          compatible={compatible.compatible}
          workspaceLifecycle="active"
          isPending={isPending}
          deployLabel={
            consumer === "claude"
              ? t("skills.library.deployClaude")
              : t("skills.library.deployCodex")
          }
          undeployLabel={
            consumer === "claude"
              ? t("skills.library.undeployClaude")
              : t("skills.library.undeployCodex")
          }
          onApply={applyDeployment}
        />
        {!compatible.compatible && (
          <span className="basis-full text-xs text-destructive">
            {compatible.issues.join("; ") ||
              t("skills.library.incompatibleConsumer", { consumer })}
          </span>
        )}
      </div>
    );
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {(libraryError ||
        projectError ||
        deploymentError ||
        globalImportsQuery.isError) && (
        <p
          role="alert"
          className="mx-5 mt-3 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        >
          {t("skills.global.loadError")}
        </p>
      )}

      <div className="px-5 pt-3">
        <GlobalSkillImportPanel
          inspection={globalImportsQuery.data}
          isLoading={globalImportsQuery.isLoading}
          isFetching={globalImportsQuery.isFetching}
          onRefetch={globalImportsQuery.refetch}
          onBusyChange={setGlobalImportBusy}
        />
      </div>

      <div className="flex flex-wrap gap-3 px-5 py-3">
        <div className="relative min-w-[16rem] flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("skills.searchPlaceholder")}
            className="pl-9"
          />
        </div>
        <Select
          value={statusFilter}
          onValueChange={(value: DeploymentStatus | "all") =>
            setStatusFilter(value)
          }
        >
          <SelectTrigger
            className="w-52"
            aria-label={t("skills.global.statusFilter")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">
              {t("skills.global.allStatuses")}
            </SelectItem>
            {statuses.map((status) => (
              <SelectItem key={status} value={status}>
                {t(`skills.library.deploymentStatus.${status}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="min-h-0 flex-1 overflow-auto px-5 pb-5">
        {isLoading || deploymentLoading ? (
          <div className="flex justify-center py-16">
            <Loader2 className="h-5 w-5 animate-spin" />
          </div>
        ) : filtered.length === 0 ? (
          <div className="rounded-xl border border-dashed p-8 text-center text-muted-foreground">
            <p className="font-medium">{t("skills.global.empty")}</p>
            <p className="mt-1 text-sm">
              {t("skills.global.emptyDescription")}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {progressiveSkills.visibleItems.map((skill) => (
              <article key={skill.id} className="rounded-xl border bg-card p-4">
                <div className="flex flex-wrap items-start gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="min-w-0 break-words font-semibold">
                        {skill.displayName}
                      </h3>
                      <Badge
                        variant="outline"
                        className="max-w-full whitespace-normal break-all text-left font-mono text-xs"
                      >
                        {skill.directory}
                      </Badge>
                    </div>
                    <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
                      <span className="min-w-0 break-all">
                        {t("skills.global.libraryId", { id: skill.id })}
                      </span>
                      {sourceSummary(skill) && (
                        <span className="min-w-0 break-all">
                          {sourceSummary(skill)}
                        </span>
                      )}
                      <Badge
                        variant={
                          skill.compatibility.claude.compatible
                            ? "secondary"
                            : "destructive"
                        }
                      >
                        {t("skills.library.consumerClaude")}:{" "}
                        {skill.compatibility.claude.compatible
                          ? t("skills.batch.compatible")
                          : t("skills.batch.incompatible")}
                      </Badge>
                      <Badge
                        variant={
                          skill.compatibility.codex.compatible
                            ? "secondary"
                            : "destructive"
                        }
                      >
                        {t("skills.library.consumerCodex")}:{" "}
                        {skill.compatibility.codex.compatible
                          ? t("skills.batch.compatible")
                          : t("skills.batch.incompatible")}
                      </Badge>
                    </div>
                  </div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2 border-t pt-3">
                  {renderConsumer(skill, "claude")}
                  {renderConsumer(skill, "codex")}
                </div>
              </article>
            ))}
            <ProgressiveSkillListFooter
              visibleCount={progressiveSkills.visibleCount}
              totalCount={progressiveSkills.totalCount}
              hasMore={progressiveSkills.hasMore}
              onShowMore={progressiveSkills.showMore}
            />
          </div>
        )}
      </div>

      <BatchDeploymentDialog
        open={batchOpen}
        onOpenChange={setBatchOpen}
        skills={filtered}
        projects={projects}
        defaultTarget={{ workspace: "global" }}
        targetLocked
        actionLocked
        defaultAction="undeploy"
        inspections={allInspections}
        isPending={batchPending || isRefreshing}
        onApply={applyBatch}
      />
    </div>
  );
});
