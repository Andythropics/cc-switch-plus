import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  useApplySkillDeployments,
  useLibrarySkills,
  useProjectWorkspaces,
  useRegisterProjectWorkspace,
  useRefreshSkillDeployments,
  useSkillDeployments,
} from "@/hooks/useSkills";
import { DeploymentStatusBadge } from "@/components/skills/DeploymentStatusBadge";
import { DeploymentResolutionActions } from "@/components/skills/DeploymentResolutionActions";
import { settingsApi } from "@/lib/api/settings";
import type {
  DeploymentConsumer,
  DeploymentIntent,
  LibrarySkill,
} from "@/lib/api/skills";
import type { ProjectWorkspace } from "@/lib/api/projectWorkspaces";

interface ProjectWorkspacesPanelProps {
  onOpenLibrary?: () => void;
}

export interface ProjectWorkspacesPanelHandle {
  refresh: () => Promise<void>;
}

function ProjectWorkspaceDeployments({
  workspace,
}: {
  workspace: ProjectWorkspace;
}) {
  const { t } = useTranslation();
  const { data: skills = [] } = useLibrarySkills();
  const { data: claudeState } = useSkillDeployments({
    consumer: "claude",
    workspace: "project",
    workspaceId: workspace.id,
  });
  const { data: codexState } = useSkillDeployments({
    consumer: "codex",
    workspace: "project",
    workspaceId: workspace.id,
  });
  const apply = useApplySkillDeployments();

  const applyDeployment = async (intent: DeploymentIntent) => {
    try {
      const result = await apply.mutateAsync({ intents: [intent] });
      const item = result.items[0];
      if (!item) throw new Error(t("skills.projects.deploymentFailed"));
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
      toast.error(String(error));
    }
  };

  const renderControl = (skill: LibrarySkill, consumer: DeploymentConsumer) => {
    const state = consumer === "claude" ? claudeState : codexState;
    const deployment = state?.items.find(
      (item) => item.librarySkillId === skill.id,
    );
    const status = deployment?.status ?? "not_deployed";
    const compatible = skill.compatibility[consumer].compatible;
    const label =
      consumer === "claude"
        ? t("skills.library.consumerClaude")
        : t("skills.library.consumerCodex");
    const incompatibilityMessage =
      skill.compatibility[consumer].issues.join("; ") ||
      t("skills.library.incompatibleConsumer", { consumer: label });
    return (
      <div key={consumer} className="flex flex-wrap items-center gap-1.5">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
          <span className="text-xs font-medium text-muted-foreground">
            {label}
          </span>
          <DeploymentStatusBadge
            status={status}
            observed={deployment?.observed}
            desired={Boolean(deployment?.desired)}
          />
        </div>
        <DeploymentResolutionActions
          skill={skill}
          target={{
            consumer,
            workspace: "project",
            workspaceId: workspace.id,
          }}
          deployment={deployment}
          compatible={compatible}
          isPending={apply.isPending}
          deployLabel={t("skills.projects.deploy")}
          undeployLabel={t("skills.projects.undeploy")}
          onApply={applyDeployment}
        />
        {!compatible && (
          <span className="basis-full text-xs text-destructive">
            {incompatibilityMessage}
          </span>
        )}
      </div>
    );
  };

  return (
    <div className="space-y-2 border-t pt-3">
      <h3 className="text-sm font-semibold">
        {t("skills.projects.deployments")}
      </h3>
      {skills.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.projects.noLibrarySkills")}
        </p>
      ) : (
        skills.map((skill) => (
          <div key={skill.id} className="rounded-lg border p-3">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="font-medium">{skill.displayName}</span>
              <span className="font-mono text-xs text-muted-foreground">
                {skill.directory}
              </span>
            </div>
            <div className="flex flex-wrap gap-2">
              {renderControl(skill, "claude")}
              {renderControl(skill, "codex")}
            </div>
          </div>
        ))
      )}
    </div>
  );
}

export const ProjectWorkspacesPanel = forwardRef<
  ProjectWorkspacesPanelHandle,
  ProjectWorkspacesPanelProps
>(function ProjectWorkspacesPanel({ onOpenLibrary }, ref) {
  const { t } = useTranslation();
  const { data: workspaces = [], isLoading } = useProjectWorkspaces();
  const register = useRegisterProjectWorkspace();
  const refreshDeployments = useRefreshSkillDeployments();
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const selected =
    workspaces.find((workspace) => workspace.id === selectedId) ??
    workspaces[0];

  const registerDirectory = async () => {
    const path = await settingsApi.pickDirectory();
    if (!path) return;
    try {
      const result = await register.mutateAsync({ path });
      setSelectedId(result.workspace.id);
      toast.success(t("skills.projects.registerSuccess"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  useImperativeHandle(ref, () => ({ refresh: refreshDeployments }), [
    refreshDeployments,
  ]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-3 border-b px-5 py-3">
        <FolderOpen className="h-5 w-5 text-primary" />
        <div className="min-w-0 flex-1">
          <h2 className="text-base font-semibold">
            {t("skills.projects.title")}
          </h2>
          <p className="text-xs text-muted-foreground">
            {t("skills.projects.description")}
          </p>
        </div>
        {onOpenLibrary && (
          <Button variant="outline" size="sm" onClick={onOpenLibrary}>
            {t("skills.projects.library")}
          </Button>
        )}
        <Button
          size="sm"
          onClick={() => void registerDirectory()}
          disabled={register.isPending}
        >
          {register.isPending && (
            <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
          )}
          {t("skills.projects.register")}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("skills.refresh")}
          title={t("skills.refresh")}
          onClick={() => void refreshDeployments()}
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-5 py-4">
        {isLoading ? (
          <div className="flex justify-center py-16">
            <Loader2 className="h-5 w-5 animate-spin" />
          </div>
        ) : workspaces.length === 0 ? (
          <div className="rounded-xl border border-dashed p-8 text-center text-muted-foreground">
            <p className="font-medium">{t("skills.projects.empty")}</p>
            <p className="mt-1 text-sm">
              {t("skills.projects.emptyDescription")}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {workspaces.map((workspace) => (
              <article
                key={workspace.id}
                className={`rounded-xl border p-4 ${selected?.id === workspace.id ? "border-primary" : ""}`}
              >
                <button
                  className="w-full text-left"
                  onClick={() => setSelectedId(workspace.id)}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-semibold">{workspace.displayName}</h3>
                    <Badge variant="outline">
                      {t(`skills.projects.rootKind.${workspace.rootKind}`)}
                    </Badge>
                    <Badge
                      variant={
                        workspace.lifecycle === "active"
                          ? "secondary"
                          : "outline"
                      }
                    >
                      {t(`skills.projects.lifecycle.${workspace.lifecycle}`)}
                    </Badge>
                  </div>
                  <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                    {workspace.rootPath}
                  </p>
                </button>
                {selected?.id === workspace.id && (
                  <ProjectWorkspaceDeployments workspace={workspace} />
                )}
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});

ProjectWorkspacesPanel.displayName = "ProjectWorkspacesPanel";
