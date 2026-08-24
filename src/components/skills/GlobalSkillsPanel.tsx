import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe2, Loader2, RefreshCw, Search } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { DeploymentRecoveryPanel } from "@/components/skills/DeploymentRecoveryPanel";
import { GlobalSkillImportPanel } from "@/components/skills/GlobalSkillImportPanel";
import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
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
  onOpenLibrary?: () => void;
  onOpenProjects?: () => void;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
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

export function GlobalSkillsPanel({
  onOpenLibrary,
  onOpenProjects,
  onInteractionBlockedChange,
  onNavigationBlockedChange,
}: GlobalSkillsPanelProps) {
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
  const apply = useApplySkillDeployments();
  const globalImportsQuery = useInspectGlobalSkillImports();
  const refreshDeployments = useRefreshSkillDeployments();
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<DeploymentStatus | "all">(
    "all",
  );
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchAction, setBatchAction] = useState<"deploy" | "undeploy">(
    "deploy",
  );
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [globalImportBusy, setGlobalImportBusy] = useState(false);

  const allInspections = useMemo(
    () => [
      ...(claudeQuery.data?.items ?? []),
      ...(codexQuery.data?.items ?? []),
    ],
    [claudeQuery.data?.items, codexQuery.data?.items],
  );
  const deploymentError = claudeQuery.isError || codexQuery.isError;
  const isRefreshing =
    libraryFetching ||
    projectsFetching ||
    claudeQuery.isFetching ||
    codexQuery.isFetching ||
    globalImportsQuery.isFetching;
  const navigationBlocked =
    apply.isPending ||
    batchOpen ||
    recoveryBusy ||
    globalImportBusy ||
    globalImportsQuery.isFetching;

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
            item.target.consumer === consumer,
        );
        return (inspection?.status ?? "not_deployed") === statusFilter;
      });
    });
  }, [allInspections, query, skills, statusFilter]);

  const applyDeployment = async (intent: DeploymentIntent) => {
    try {
      const result = await apply.mutateAsync({ intents: [intent] });
      const item = result.items[0];
      if (!item) throw new Error(t("skills.library.deploymentFailed"));
      const label = t(deploymentOutcomeLabelKeys[item.outcome]);
      if (successfulDeploymentOutcomes.has(item.outcome)) {
        toast.success(item.message ? `${label}: ${item.message}` : label);
      } else {
        toast.error(item.message ? `${label}: ${item.message}` : label);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const applyBatch = async (batch: DeploymentBatch) => {
    const result = await apply.mutateAsync(batch);
    await Promise.all([
      refreshDeployments(),
      refetchLibrary(),
      refetchProjects(),
      globalImportsQuery.refetch(),
    ]);
    return result;
  };

  const refresh = async () => {
    await Promise.all([
      refreshDeployments(),
      refetchLibrary(),
      refetchProjects(),
      globalImportsQuery.refetch(),
    ]);
  };

  const renderConsumer = (
    skill: LibrarySkill,
    consumer: DeploymentConsumer,
  ) => {
    const state = consumer === "claude" ? claudeQuery.data : codexQuery.data;
    const deployment = state?.items.find(
      (item) => item.librarySkillId === skill.id,
    );
    const compatible = skill.compatibility[consumer];
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
          desired={Boolean(deployment?.desired)}
        />
        <DeploymentResolutionActions
          skill={skill}
          target={{ consumer, workspace: "global" }}
          deployment={deployment}
          compatible={compatible.compatible}
          workspaceLifecycle="active"
          isPending={apply.isPending}
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
      <div className="flex flex-wrap items-center gap-3 border-b px-5 py-3">
        <Globe2 className="h-5 w-5 text-primary" />
        <div className="min-w-0 flex-1">
          <h2 className="text-base font-semibold">
            {t("skills.global.title")}
          </h2>
          <p className="text-xs text-muted-foreground">
            {t("skills.global.description")}
          </p>
        </div>
        {onOpenLibrary && (
          <Button
            variant="outline"
            size="sm"
            disabled={navigationBlocked}
            onClick={onOpenLibrary}
          >
            {t("skills.global.library")}
          </Button>
        )}
        {onOpenProjects && (
          <Button
            variant="outline"
            size="sm"
            disabled={navigationBlocked}
            onClick={onOpenProjects}
          >
            {t("skills.global.projects")}
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          disabled={apply.isPending || isRefreshing || skills.length === 0}
          onClick={() => {
            setBatchAction("deploy");
            setBatchOpen(true);
          }}
        >
          {t("skills.global.batchDeploy")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={apply.isPending || isRefreshing || skills.length === 0}
          onClick={() => {
            setBatchAction("undeploy");
            setBatchOpen(true);
          }}
        >
          {t("skills.global.batchUndeploy")}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("skills.refresh")}
          title={t("skills.refresh")}
          disabled={isRefreshing || apply.isPending}
          onClick={() => void refresh()}
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

      <div className="px-5 pt-3">
        <DeploymentRecoveryPanel
          query={{ workspace: "global" }}
          onBusyChange={setRecoveryBusy}
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
        {isLoading ? (
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
            {filtered.map((skill) => (
              <article key={skill.id} className="rounded-xl border bg-card p-4">
                <div className="flex flex-wrap items-start gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="font-semibold">{skill.displayName}</h3>
                      <Badge variant="outline" className="font-mono text-xs">
                        {skill.directory}
                      </Badge>
                    </div>
                    <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
                      <span>
                        {t("skills.global.libraryId", { id: skill.id })}
                      </span>
                      {sourceSummary(skill) && (
                        <span>{sourceSummary(skill)}</span>
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
        defaultAction={batchAction}
        inspections={allInspections}
        isPending={apply.isPending || isRefreshing}
        onApply={applyBatch}
      />
    </div>
  );
}
