import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkillsDialogContent } from "./SkillsDialogContent";
import { useLinkLibrarySkillSource } from "@/hooks/useExternalSkillUpdates";
import { sourceFromInput } from "@/lib/skillSource";
import type { LibrarySkill } from "@/lib/api/skills";

/** Mounted only for the card being edited; source linking never scans the Library. */
export function LinkSkillSourceDialog({
  skill,
  onClose,
}: {
  skill: LibrarySkill;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const source = skill.source;
  const initialUrl =
    source.repoOwner && source.repoName
      ? `https://github.com/${source.repoOwner}/${source.repoName}`
      : "";
  const [url, setUrl] = useState(initialUrl);
  const [branch, setBranch] = useState(source.repoBranch ?? "");
  const [path, setPath] = useState(source.skillPath ?? (initialUrl ? "." : ""));
  const [error, setError] = useState<string | null>(null);
  const mutation = useLinkLibrarySkillSource();
  const dirty =
    url !== initialUrl ||
    branch !== (source.repoBranch ?? "") ||
    path !== (source.skillPath ?? (initialUrl ? "." : ""));
  const save = async () => {
    if (mutation.isPending) return;
    setError(null);
    let nextSource;
    try {
      nextSource = sourceFromInput(url, branch, path);
      if (url === initialUrl && source.kind === "marketplace") {
        nextSource.kind = "marketplace";
        nextSource.marketplace = source.marketplace;
      }
    } catch {
      setError(t("skills.external.invalidSource"));
      return;
    }
    try {
      await mutation.mutateAsync({
        librarySkillId: skill.id,
        source: nextSource,
        expectedContentHash: skill.contentHash,
      });
      toast.success(t("skills.external.linked"));
      onClose();
    } catch (cause) {
      setError(String(cause));
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !mutation.isPending) onClose();
      }}
    >
      <SkillsDialogContent closeBlocked={mutation.isPending || dirty}>
        <DialogHeader>
          <DialogTitle>{t("skills.external.manual")}</DialogTitle>
          <DialogDescription>
            {t("skills.external.cardLinkDescription", {
              skill: skill.displayName,
            })}
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-4">
          <p className="font-medium">
            {skill.displayName}{" "}
            <span className="text-muted-foreground">({skill.directory})</span>
          </p>
          {error && (
            <p role="alert" className="break-words text-destructive">
              {error}
            </p>
          )}
          <div className="space-y-2">
            <Label htmlFor="skill-source-url">
              {t("skills.external.sourceUrl")}
            </Label>
            <Input
              id="skill-source-url"
              value={url}
              disabled={mutation.isPending}
              placeholder="https://github.com/owner/repo"
              onChange={(event) => setUrl(event.target.value)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="skill-source-branch">
              {t("skills.external.branch")}
            </Label>
            <Input
              id="skill-source-branch"
              value={branch}
              disabled={mutation.isPending}
              onChange={(event) => setBranch(event.target.value)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="skill-source-path">
              {t("skills.external.path")}
            </Label>
            <Input
              id="skill-source-path"
              value={path}
              disabled={mutation.isPending}
              placeholder="skills/implementation"
              onChange={(event) => setPath(event.target.value)}
            />
          </div>
        </DialogBody>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={mutation.isPending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button
            disabled={mutation.isPending || !url.trim() || !path.trim()}
            onClick={() => void save()}
          >
            {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("skills.external.linkOnly")}
          </Button>
        </DialogFooter>
      </SkillsDialogContent>
    </Dialog>
  );
}
