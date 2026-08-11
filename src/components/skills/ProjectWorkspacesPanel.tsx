import { useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Link2, Loader2, Unlink } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  useApplySkillDeployments,
  useLibrarySkills,
  useProjectWorkspaces,
  useRegisterProjectWorkspace,
  useSkillDeployments,
} from "@/hooks/useSkills";
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

  const applyDeployment = async (
    skill: LibrarySkill,
    consumer: DeploymentConsumer,
    action: Extract<
      DeploymentIntent,
      { action: "deploy" | "undeploy" }
    >["action"],
  ) => {
    try {
      const result = await apply.mutateAsync({
        intents: [
          {
            action,
            librarySkillId: skill.id,
            target: {
              consumer,
              workspace: "project",
              workspaceId: workspace.id,
            },
          },
        ],
      });
      const item = result.items[0];
      if (item?.outcome === "error")
        throw new Error(item.message || t("skills.projects.deploymentFailed"));
      if (item?.outcome === "conflict") {
        toast.error(t("skills.projects.deploymentConflict"));
        return;
      }
      toast.success(
        t(
          action === "deploy"
            ? "skills.projects.deploySuccess"
            : "skills.projects.undeploySuccess",
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
    const blocked =
      !compatible ||
      ["conflict", "orphaned", "unsupported", "blocked"].includes(status);
    const deployed = status === "in_sync";
    const label =
      consumer === "claude"
        ? t("skills.library.consumerClaude")
        : t("skills.library.consumerCodex");
    const incompatibilityMessage =
      skill.compatibility[consumer].issues.join("; ") ||
      t("skills.library.incompatibleConsumer", { consumer: label });
    return (
      <div key={consumer} className="flex flex-wrap items-center gap-1.5">
        <Badge variant={status === "in_sync" ? "secondary" : "outline"}>
          {label}: {t(`skills.library.deploymentStatus.${status}`)}
        </Badge>
        <Button
          variant="outline"
          size="sm"
          disabled={apply.isPending || (deployed ? false : blocked)}
          onClick={() =>
            void applyDeployment(
              skill,
              consumer,
              deployed ? "undeploy" : "deploy",
            )
          }
          title={
            !compatible
              ? skill.compatibility[consumer].issues.join("; ")
              : undefined
          }
        >
          {deployed ? (
            <Unlink className="mr-1 h-3.5 w-3.5" />
          ) : (
            <Link2 className="mr-1 h-3.5 w-3.5" />
          )}
          {deployed
            ? t("skills.projects.undeploy")
            : t("skills.projects.deploy")}
        </Button>
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

export function ProjectWorkspacesPanel({
  onOpenLibrary,
}: ProjectWorkspacesPanelProps) {
  const { t } = useTranslation();
  const { data: workspaces = [], isLoading } = useProjectWorkspaces();
  const register = useRegisterProjectWorkspace();
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
                {selected?.id === workspace.id &&
                  workspace.lifecycle === "active" && (
                    <ProjectWorkspaceDeployments workspace={workspace} />
                  )}
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
