import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Trash2, ExternalLink, Loader2, Plus } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";
import {
  getSkillTechnicalDetails,
  SkillTechnicalDetails,
} from "@/components/skills/SkillTechnicalDetails";

interface RepoManagerPanelProps {
  repos: SkillRepo[];
  skills: DiscoverableSkill[];
  onAdd: (repo: SkillRepo) => Promise<void>;
  onRemove: (owner: string, name: string) => Promise<void>;
  onClose: () => void;
}

export function RepoManagerPanel({
  repos,
  skills,
  onAdd,
  onRemove,
  onClose,
}: RepoManagerPanelProps) {
  const { t } = useTranslation();
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [error, setError] = useState("");
  const [errorDetails, setErrorDetails] = useState("");
  const [pendingRemoval, setPendingRemoval] = useState<SkillRepo | null>(null);
  const [isRemoving, setIsRemoving] = useState(false);

  const getSkillCount = (repo: SkillRepo) =>
    skills.filter(
      (skill) =>
        skill.repoOwner === repo.owner &&
        skill.repoName === repo.name &&
        (skill.repoBranch || "main") === (repo.branch || "main"),
    ).length;

  const parseRepoUrl = (
    url: string,
  ): { owner: string; name: string } | null => {
    let cleaned = url.trim();
    cleaned = cleaned.replace(/^https?:\/\/github\.com\//, "");
    cleaned = cleaned.replace(/\.git$/, "");

    const parts = cleaned.split("/");
    if (parts.length === 2 && parts[0] && parts[1]) {
      return { owner: parts[0], name: parts[1] };
    }

    return null;
  };

  const handleAdd = async () => {
    setError("");
    setErrorDetails("");

    const parsed = parseRepoUrl(repoUrl);
    if (!parsed) {
      setError(t("skills.repo.invalidUrl"));
      return;
    }

    try {
      await onAdd({
        owner: parsed.owner,
        name: parsed.name,
        branch: branch || "main",
        enabled: true,
      });

      setRepoUrl("");
      setBranch("");
    } catch (e) {
      setError(t("skills.repo.addFailed"));
      setErrorDetails(getSkillTechnicalDetails(e));
    }
  };

  const handleOpenRepo = async (owner: string, name: string) => {
    try {
      await settingsApi.openExternal(`https://github.com/${owner}/${name}`);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  const handleRemove = async () => {
    if (!pendingRemoval || isRemoving) return;
    setIsRemoving(true);
    try {
      await onRemove(pendingRemoval.owner, pendingRemoval.name);
      setPendingRemoval(null);
    } catch {
      // The parent owns the localized toast. Keep the confirmation open so
      // the user can retry or cancel after a failed removal.
    } finally {
      setIsRemoving(false);
    }
  };

  return (
    <FullScreenPanel
      isOpen={true}
      title={t("skills.repo.title")}
      onClose={onClose}
    >
      {/* 添加仓库表单 */}
      <div className="space-y-4 glass-card rounded-xl p-6">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.addRepo")}
        </h3>
        <div className="space-y-4">
          <div>
            <Label htmlFor="repo-url" className="text-foreground">
              {t("skills.repo.url")}
            </Label>
            <Input
              id="repo-url"
              placeholder={t("skills.repo.urlPlaceholder")}
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              className="mt-2"
            />
          </div>
          <div>
            <Label htmlFor="branch" className="text-foreground">
              {t("skills.repo.branch")}
            </Label>
            <Input
              id="branch"
              placeholder={t("skills.repo.branchPlaceholder")}
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              className="mt-2"
            />
          </div>
          {error && (
            <div className="text-sm text-red-600 dark:text-red-400">
              <p>{error}</p>
              <SkillTechnicalDetails details={errorDetails} />
            </div>
          )}
          <Button
            onClick={handleAdd}
            className="bg-primary text-primary-foreground hover:bg-primary/90"
            type="button"
          >
            <Plus className="h-4 w-4 mr-2" />
            {t("skills.repo.add")}
          </Button>
        </div>
      </div>

      {/* 仓库列表 */}
      <div className="space-y-4">
        <h3 className="text-base font-semibold text-foreground">
          {t("skills.repo.list")}
        </h3>
        {repos.length === 0 ? (
          <div className="text-center py-12 glass-card rounded-xl">
            <p className="text-sm text-muted-foreground">
              {t("skills.repo.empty")}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {repos.map((repo) => (
              <div
                key={`${repo.owner}/${repo.name}`}
                className="flex items-center justify-between glass-card rounded-xl px-4 py-3"
              >
                <div>
                  <div className="text-sm font-medium text-foreground">
                    {repo.owner}/{repo.name}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("skills.repo.branch")}: {repo.branch || "main"}
                    <span className="ml-3 inline-flex items-center rounded-full border border-border-default px-2 py-0.5 text-[11px]">
                      {t("skills.repo.skillCount", {
                        count: getSkillCount(repo),
                      })}
                    </span>
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button
                    variant="ghost"
                    size="icon"
                    type="button"
                    onClick={() => handleOpenRepo(repo.owner, repo.name)}
                    title={t("common.view", { defaultValue: "查看" })}
                    className="hover:bg-black/5 dark:hover:bg-white/5"
                  >
                    <ExternalLink className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    type="button"
                    onClick={() => setPendingRemoval(repo)}
                    aria-label={t("skills.repo.removeAction", {
                      repository: `${repo.owner}/${repo.name}`,
                    })}
                    title={t("skills.repo.removeAction", {
                      repository: `${repo.owner}/${repo.name}`,
                    })}
                    className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <Dialog
        open={pendingRemoval !== null}
        onOpenChange={(open) => {
          if (!open && !isRemoving) setPendingRemoval(null);
        }}
      >
        <DialogContent zIndex="top">
          <DialogHeader>
            <DialogTitle>{t("skills.repo.removeTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.repo.removeDescription")}
            </DialogDescription>
          </DialogHeader>
          {pendingRemoval && (
            <DialogBody className="space-y-3 text-sm">
              <p className="break-all font-mono font-medium">
                {pendingRemoval.owner}/{pendingRemoval.name}
              </p>
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-2 text-muted-foreground">
                <dt>{t("skills.repo.branch")}</dt>
                <dd className="break-all font-mono">
                  {pendingRemoval.branch || "main"}
                </dd>
                <dt>{t("skills.repo.affectedSkills")}</dt>
                <dd>{getSkillCount(pendingRemoval)}</dd>
              </dl>
            </DialogBody>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={isRemoving}
              onClick={() => setPendingRemoval(null)}
            >
              {t("skills.repo.removeCancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={isRemoving}
              onClick={() => void handleRemove()}
            >
              {isRemoving && <Loader2 className="h-4 w-4 animate-spin" />}
              {t("skills.repo.removeConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </FullScreenPanel>
  );
}
