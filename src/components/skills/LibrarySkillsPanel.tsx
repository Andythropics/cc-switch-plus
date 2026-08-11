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
  Link2,
  Pencil,
  Search,
  Unlink,
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
  useLibrarySkills,
  useSkillDeployments,
  useUpdateLibrarySkillMetadata,
} from "@/hooks/useSkills";
import { skillsApi } from "@/lib/api";
import type {
  ConsumerCompatibility,
  DeploymentConsumer,
  DeploymentIntent,
  LibrarySkill,
} from "@/lib/api/skills";

interface LibrarySkillsPanelProps {
  onOpenDiscovery: () => void;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

export interface LibrarySkillsPanelHandle {
  openDiscovery: () => void;
  openAcquireFromZip: () => Promise<void>;
}

const sourceSummary = (skill: LibrarySkill) => {
  if (skill.source.repoOwner && skill.source.repoName) {
    return `${skill.source.repoOwner}/${skill.source.repoName}`;
  }
  return skill.source.url;
};

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
    { onOpenDiscovery, onInteractionBlockedChange, onNavigationBlockedChange },
    ref,
  ) => {
    const { t } = useTranslation();
    const { data: skills = [], isLoading } = useLibrarySkills();
    const { data: deploymentState } = useSkillDeployments({
      consumer: "claude",
      workspace: "global",
    });
    const { data: codexDeploymentState } = useSkillDeployments({
      consumer: "codex",
      workspace: "global",
    });
    const updateMetadata = useUpdateLibrarySkillMetadata();
    const acquireZip = useAcquireLibrarySkillsFromZip();
    const applyDeployments = useApplySkillDeployments();
    const [query, setQuery] = useState("");
    const [editing, setEditing] = useState<LibrarySkill | null>(null);
    const [displayName, setDisplayName] = useState("");
    const [description, setDescription] = useState("");
    const [zipCollision, setZipCollision] = useState<{
      filePath: string;
      directory: string;
    } | null>(null);
    const [uniqueDirectory, setUniqueDirectory] = useState("");

    const blocked =
      updateMetadata.isPending ||
      acquireZip.isPending ||
      applyDeployments.isPending ||
      editing !== null ||
      zipCollision !== null;

    useEffect(() => {
      onInteractionBlockedChange?.(blocked);
      onNavigationBlockedChange?.(blocked);
    }, [blocked, onInteractionBlockedChange, onNavigationBlockedChange]);

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

    const applyGlobalDeployment = async (
      skill: LibrarySkill,
      consumer: DeploymentConsumer,
      action: Extract<
        DeploymentIntent,
        { action: "deploy" | "undeploy" }
      >["action"],
    ) => {
      try {
        const result = await applyDeployments.mutateAsync({
          intents: [
            {
              action,
              librarySkillId: skill.id,
              target: { consumer, workspace: "global" },
            },
          ],
        });
        const item = result.items[0];
        if (item?.outcome === "error") {
          throw new Error(item.message || t("skills.library.deploymentFailed"));
        }
        if (item?.outcome === "conflict") {
          toast.error(t("skills.library.deploymentConflict"));
          return;
        }
        toast.success(
          action === "deploy"
            ? t(
                consumer === "claude"
                  ? "skills.library.deploySuccess"
                  : "skills.library.deployCodexSuccess",
              )
            : t(
                consumer === "claude"
                  ? "skills.library.undeploySuccess"
                  : "skills.library.undeployCodexSuccess",
              ),
        );
      } catch (error) {
        toast.error(String(error));
      }
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
      const isDeployed = status === "in_sync";
      const isBlocked =
        !compatibility.compatible ||
        status === "conflict" ||
        status === "orphaned" ||
        status === "unsupported" ||
        status === "blocked";
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
          className="flex min-w-[16rem] flex-1 flex-wrap items-center gap-2"
          data-testid={`deployment-control-${consumer}`}
        >
          <Badge
            variant={status === "in_sync" ? "secondary" : "outline"}
            title={
              !compatibility.compatible ? incompatibilityMessage : undefined
            }
          >
            {t(`skills.library.deploymentStatus.${status}`)}
          </Badge>
          {isDeployed ? (
            <Button
              variant="outline"
              size="sm"
              disabled={blocked || applyDeployments.isPending}
              onClick={() =>
                void applyGlobalDeployment(skill, consumer, "undeploy")
              }
            >
              <Unlink className="mr-1.5 h-3.5 w-3.5" />
              {undeployLabel}
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              title={
                isBlocked && !compatibility.compatible
                  ? incompatibilityMessage
                  : undefined
              }
              disabled={blocked || isBlocked || applyDeployments.isPending}
              onClick={() =>
                void applyGlobalDeployment(skill, consumer, "deploy")
              }
            >
              <Link2 className="mr-1.5 h-3.5 w-3.5" />
              {deployLabel}
            </Button>
          )}
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
          <Button variant="outline" size="sm" onClick={onOpenDiscovery}>
            {t("skills.discover")}
          </Button>
        </div>

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
                                : "skills.library.sourceMarketplace",
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
                  </div>
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
