import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Archive,
  ArchiveRestore,
  FolderOpen,
  Loader2,
  MapPin,
  Pencil,
  RefreshCw,
  Trash2,
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
import {
  useArchiveProjectWorkspace,
  useApplySkillDeployments,
  useForgetProjectWorkspace,
  useLibrarySkills,
  useProjectWorkspaces,
  useRegisterProjectWorkspace,
  useRelocateProjectWorkspace,
  useRenameProjectWorkspace,
  useRestoreProjectWorkspace,
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
          workspaceLifecycle={workspace.lifecycle}
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
  const projectQuery = useProjectWorkspaces();
  const workspaces = projectQuery.data ?? [];
  const isLoading = projectQuery.isLoading;
  const refetchWorkspaces =
    projectQuery.refetch ?? (async () => ({ data: workspaces }));
  const register = useRegisterProjectWorkspace();
  const rename = useRenameProjectWorkspace();
  const archive = useArchiveProjectWorkspace();
  const restore = useRestoreProjectWorkspace();
  const relocate = useRelocateProjectWorkspace();
  const forget = useForgetProjectWorkspace();
  const refreshDeployments = useRefreshSkillDeployments();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);
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

  const activeWorkspaces = workspaces.filter(
    (workspace) => workspace.lifecycle !== "archived",
  );
  const archivedWorkspaces = workspaces.filter(
    (workspace) => workspace.lifecycle === "archived",
  );
  const lifecycleBusy =
    register.isPending ||
    rename.isPending ||
    archive.isPending ||
    restore.isPending ||
    relocate.isPending ||
    forget.isPending;

  const selected =
    workspaces.find((workspace) => workspace.id === selectedId) ??
    activeWorkspaces[0] ??
    (showArchived ? archivedWorkspaces[0] : undefined);

  const refreshAll = async () => {
    await Promise.all([refetchWorkspaces(), refreshDeployments()]);
  };

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
      toast.error(String(error));
    }
  };

  const submitArchive = async () => {
    if (!archiveTarget) return;
    try {
      await archive.mutateAsync(archiveTarget.id);
      setArchiveTarget(null);
      toast.success(t("skills.projects.archiveSuccess"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  const restoreWorkspace = async (workspace: ProjectWorkspace) => {
    try {
      await restore.mutateAsync(workspace.id);
      setSelectedId(workspace.id);
      setShowArchived(false);
      toast.success(t("skills.projects.restoreSuccess"));
    } catch (error) {
      toast.error(String(error));
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
      toast.error(String(error));
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
      const message = String(error);
      toast.error(`${t("skills.projects.forgetBlocked")}: ${message}`);
    }
  };

  useImperativeHandle(ref, () => ({ refresh: refreshAll }), [refreshAll]);

  const renderWorkspace = (workspace: ProjectWorkspace) => {
    const selectedWorkspace = selected?.id === workspace.id;
    return (
      <article
        key={workspace.id}
        className={`rounded-xl border p-4 ${selectedWorkspace ? "border-primary" : ""}`}
        data-testid={`project-workspace-${workspace.lifecycle}`}
      >
        <div className="flex items-start gap-3">
          <button
            className="min-w-0 flex-1 text-left"
            onClick={() => setSelectedId(workspace.id)}
          >
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-semibold">{workspace.displayName}</h3>
              <Badge variant="outline">
                {t(`skills.projects.rootKind.${workspace.rootKind}`)}
              </Badge>
              <Badge
                variant={
                  workspace.lifecycle === "active" ? "secondary" : "outline"
                }
              >
                {t(`skills.projects.lifecycle.${workspace.lifecycle}`)}
              </Badge>
            </div>
            <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
              {workspace.rootPath}
            </p>
          </button>
          <div className="flex shrink-0 flex-wrap justify-end gap-1.5">
            <Button
              variant="ghost"
              size="icon"
              aria-label={t("skills.projects.rename")}
              title={t("skills.projects.rename")}
              disabled={lifecycleBusy}
              onClick={() => openRename(workspace)}
            >
              <Pencil className="h-4 w-4" />
            </Button>
            {workspace.lifecycle !== "archived" && (
              <Button
                variant="outline"
                size="sm"
                disabled={lifecycleBusy}
                onClick={() => setArchiveTarget(workspace)}
              >
                <Archive className="mr-1 h-3.5 w-3.5" />
                {t("skills.projects.archive")}
              </Button>
            )}
            {workspace.lifecycle === "archived" && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={lifecycleBusy}
                  onClick={() => void restoreWorkspace(workspace)}
                >
                  <ArchiveRestore className="mr-1 h-3.5 w-3.5" />
                  {t("skills.projects.restore")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={lifecycleBusy}
                  onClick={() => setForgetTarget(workspace)}
                >
                  <Trash2 className="mr-1 h-3.5 w-3.5" />
                  {t("skills.projects.forget")}
                </Button>
              </>
            )}
            {workspace.lifecycle === "unavailable" && (
              <Button
                variant="outline"
                size="sm"
                disabled={lifecycleBusy}
                onClick={() => void openRelocate(workspace)}
              >
                <MapPin className="mr-1 h-3.5 w-3.5" />
                {t("skills.projects.relocate")}
              </Button>
            )}
          </div>
        </div>
        {selectedWorkspace && (
          <ProjectWorkspaceDeployments workspace={workspace} />
        )}
      </article>
    );
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
          disabled={lifecycleBusy}
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
          onClick={() => void refreshAll()}
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
                  <Archive className="mr-1.5 h-4 w-4" />
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
        open={Boolean(renameTarget)}
        onOpenChange={(open) => !open && setRenameTarget(null)}
      >
        <DialogContent>
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
            <Button variant="outline" onClick={() => setRenameTarget(null)}>
              {t("skills.projects.cancel")}
            </Button>
            <Button
              onClick={() => void submitRename()}
              disabled={!renameName.trim() || rename.isPending}
            >
              {t("skills.projects.renameConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(archiveTarget)}
        onOpenChange={(open) => !open && setArchiveTarget(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("skills.projects.archiveTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.archiveDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setArchiveTarget(null)}>
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
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(relocateTarget)}
        onOpenChange={(open) => !open && setRelocateTarget(null)}
      >
        <DialogContent>
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
            <Button variant="outline" onClick={() => setRelocateTarget(null)}>
              {t("skills.projects.cancel")}
            </Button>
            <Button
              onClick={() => void submitRelocate()}
              disabled={relocate.isPending || !relocatePath}
            >
              {t("skills.projects.relocateConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(forgetTarget)}
        onOpenChange={(open) => !open && setForgetTarget(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("skills.projects.forgetTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.projects.forgetDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForgetTarget(null)}>
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
        </DialogContent>
      </Dialog>
    </div>
  );
});

ProjectWorkspacesPanel.displayName = "ProjectWorkspacesPanel";
