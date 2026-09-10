import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Globe2, Loader2 } from "lucide-react";
import { toast } from "sonner";

import {
  WorkspaceSkillCard,
  type WorkspaceDeploymentControl,
  workspaceSkillGridClassName,
  sourceSummary,
} from "@/components/skills/WorkspaceSkillCard";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { BatchDeploymentDialog } from "@/components/skills/BatchDeploymentDialog";
import { GlobalSkillImportPanel } from "@/components/skills/GlobalSkillImportPanel";
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

export const GlobalSkillsPanel = forwardRef<
  GlobalSkillsPanelHandle,
  GlobalSkillsPanelProps
>(({ onInteractionBlockedChange }, ref) => {
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
  }, [navigationBlocked, onInteractionBlockedChange]);

  useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
    },
    [onInteractionBlockedChange],
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

  const getDeploymentControl = (
    skill: LibrarySkill,
    consumer: DeploymentConsumer,
  ): WorkspaceDeploymentControl => {
    const state = consumer === "claude" ? claudeQuery.data : codexQuery.data;
    return {
      skill,
      target: { consumer, workspace: "global" },
      deployment: state?.items.find((item) => item.librarySkillId === skill.id),
      compatible: skill.compatibility[consumer].compatible,
      workspaceLifecycle: "active",
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
        <Select
          value={statusFilter}
          onValueChange={(value: DeploymentStatus | "all") =>
            setStatusFilter(value)
          }
        >
          <SelectTrigger
            className="w-full sm:w-52"
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

      <div className="px-5 pb-3">
        <GlobalSkillImportPanel
          inspection={globalImportsQuery.data}
          isLoading={globalImportsQuery.isLoading}
          isFetching={globalImportsQuery.isFetching}
          onRefetch={globalImportsQuery.refetch}
          onBusyChange={setGlobalImportBusy}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto px-5 pb-5">
        {isLoading || deploymentLoading ? (
          <div className="flex justify-center py-16">
            <Loader2 className="h-5 w-5 animate-spin" />
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-16 text-center text-muted-foreground">
            <Globe2 className="h-10 w-10 opacity-50" />
            <p className="font-medium">{t("skills.global.empty")}</p>
            <p className="max-w-sm text-sm">
              {t("skills.global.emptyDescription")}
            </p>
          </div>
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
