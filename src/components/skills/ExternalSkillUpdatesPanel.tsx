import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
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
import { useExternalSkillUpdates } from "@/hooks/useExternalSkillUpdates";
import type { ExternalSkillCandidate } from "@/lib/api/externalSkillUpdates";
import type { LibrarySkill } from "@/lib/api/skills";

interface Props {
  skills: LibrarySkill[];
  disabled?: boolean;
  onInteractionBlockedChange?: (blocked: boolean) => void;
}

function recommendTarget(
  candidate: ExternalSkillCandidate,
  skills: LibrarySkill[],
) {
  const suggestions = skills.filter((skill) =>
    candidate.suggestedLibrarySkillIds.includes(skill.id),
  );
  const exact = suggestions.filter(
    (skill) => skill.directory === candidate.directory,
  );
  const best = exact.length ? exact : suggestions;
  return {
    skill: best.length === 1 ? best[0] : undefined,
    reason: exact.length ? "directory" : "name",
    suggestions,
  };
}

export function ExternalSkillUpdatesPanel({
  skills,
  disabled = false,
  onInteractionBlockedChange,
}: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [selection, setSelection] = useState<{
    candidate: ExternalSkillCandidate;
    token: string;
  } | null>(null);
  const [targetId, setTargetId] = useState("");
  const [confirmLocal, setConfirmLocal] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const { inspection, link, apply } = useExternalSkillUpdates();
  const busy = link.isPending || apply.isPending;
  const candidates = (inspection.data?.candidates ?? []).filter(
    (candidate) => !candidate.fullySynced,
  );
  const target = skills.find((skill) => skill.id === targetId);
  const recommendation =
    selection && !selection.candidate.librarySkillId
      ? recommendTarget(selection.candidate, skills)
      : null;
  const orderedSkills = recommendation
    ? [...skills].sort((a, b) => {
        const rank = (skill: LibrarySkill) =>
          skill.id === recommendation.skill?.id
            ? 2
            : recommendation.suggestions.some((item) => item.id === skill.id)
              ? 1
              : 0;
        return rank(b) - rank(a);
      })
    : skills;

  useEffect(() => {
    onInteractionBlockedChange?.(open);
    return () => onInteractionBlockedChange?.(false);
  }, [open, onInteractionBlockedChange]);

  const resetSelection = () => {
    setSelection(null);
    setTargetId("");
    setConfirmLocal(false);
    setMessage(null);
  };
  const choose = (candidate: ExternalSkillCandidate) => {
    setSelection({ candidate, token: inspection.data!.observationToken });
    // A recommendation remains editable and never executes an association.
    setTargetId(
      candidate.librarySkillId ??
        recommendTarget(candidate, skills).skill?.id ??
        "",
    );
    setConfirmLocal(false);
    setMessage(null);
  };
  const run = async (update: boolean) => {
    if (!selection || !target || busy) return;
    const intent = {
      candidateId: selection.candidate.id,
      librarySkillId: target.id,
      observationToken:
        selection.candidate.targetObservationTokens?.[target.id] ??
        selection.token,
      confirmLocalModifications: confirmLocal,
      restoreDeployment: update,
    };
    setMessage(null);
    try {
      let successMessage = t(
        update ? "skills.external.updated" : "skills.external.linked",
      );
      if (update) {
        const result = await apply.mutateAsync(intent);
        const details = [
          result.message,
          result.backupPath
            ? t("skills.external.backup", { path: result.backupPath })
            : null,
        ]
          .filter(Boolean)
          .join("\n");
        if (!["updated", "up_to_date"].includes(result.outcome)) {
          setMessage(
            `${t(`skills.library.update.applyOutcome.${result.outcome}`)}${details ? `: ${details}` : ""}`,
          );
          return;
        }
        if (details) successMessage += `\n${details}`;
      } else {
        await link.mutateAsync(intent);
      }
      resetSelection();
      setMessage(successMessage);
    } catch (error) {
      setMessage(String(error));
    }
  };
  return (
    <>
      <Button
        size="sm"
        variant="outline"
        className="shrink-0"
        disabled={disabled || busy}
        onClick={() => {
          resetSelection();
          setOpen(true);
          void inspection.refetch();
        }}
      >
        <Download className="h-4 w-4" />
        {t("skills.external.syncCli")}
      </Button>
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!busy) {
            setOpen(value);
            if (!value) resetSelection();
          }
        }}
      >
        <SkillsDialogContent
          closeBlocked={busy || Boolean(selection)}
          className="max-w-2xl"
        >
          <DialogHeader>
            <DialogTitle>{t("skills.external.syncCli")}</DialogTitle>
            <DialogDescription>
              {t("skills.external.cliDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-4 overflow-y-auto">
            {message && (
              <p
                role="alert"
                className="whitespace-pre-wrap break-words rounded border p-3"
              >
                {message}
              </p>
            )}
            {!selection ? (
              <>
                <Button
                  variant="outline"
                  disabled={inspection.isFetching}
                  onClick={() => void inspection.refetch()}
                >
                  {t("skills.external.rescan")}
                </Button>
                {inspection.isError && (
                  <p role="alert" className="text-destructive">
                    {t("skills.external.scanError")}
                  </p>
                )}
                {inspection.data?.warnings.map((warning, index) => (
                  <p
                    key={index}
                    role="alert"
                    className="break-words text-muted-foreground"
                  >
                    {warning}
                  </p>
                ))}
                {inspection.isFetching && (
                  <p role="status" className="flex items-center gap-2">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    {t("skills.external.scanning")}
                  </p>
                )}
                {!inspection.isFetching &&
                  !inspection.isError &&
                  candidates.length === 0 && (
                    <p className="text-muted-foreground">
                      {t(
                        inspection.data?.warnings.length
                          ? "skills.external.empty"
                          : "skills.external.allSynced",
                      )}
                    </p>
                  )}
                {!inspection.isFetching &&
                  !inspection.isError &&
                  candidates.map((candidate) => (
                    <div
                      key={candidate.id}
                      className="flex items-center justify-between gap-3 rounded-md border p-3"
                    >
                      <div className="min-w-0">
                        <p className="font-medium">{candidate.directory}</p>
                        <p className="break-all text-xs text-muted-foreground">
                          {candidate.source.repoOwner}/
                          {candidate.source.repoName} ·{" "}
                          {candidate.source.skillPath || "."}
                        </p>
                        <p className="text-xs">
                          {t(
                            !candidate.librarySkillId
                              ? "skills.external.unlinked"
                              : candidate.changed ||
                                  candidate.deploymentReplaced
                                ? "skills.external.changed"
                                : "skills.external.current",
                          )}
                        </p>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => choose(candidate)}
                      >
                        {t("skills.external.select")}
                      </Button>
                    </div>
                  ))}
              </>
            ) : (
              <>
                {selection && (
                  <div className="rounded-md border p-3">
                    <p className="font-medium">
                      {selection.candidate.directory}
                    </p>
                    <p className="break-all">
                      {selection.candidate.source.repoOwner}/
                      {selection.candidate.source.repoName} ·{" "}
                      {selection.candidate.source.skillPath || "."}
                    </p>
                    <p className="break-all text-muted-foreground">
                      {t("skills.external.incomingBranch", {
                        branch:
                          selection.candidate.source.repoBranch ||
                          t("skills.external.defaultBranch"),
                      })}
                    </p>
                    <p className="mt-2 text-muted-foreground">
                      {t("skills.external.identityHint")}
                    </p>
                    {selection.candidate.deploymentReplaced && (
                      <p>{t("skills.external.restoreHint")}</p>
                    )}
                  </div>
                )}
                <div className="space-y-2">
                  <Label htmlFor="external-library-target">
                    {t("skills.external.target")}
                  </Label>
                  <select
                    id="external-library-target"
                    className="h-10 w-full rounded-md border bg-background px-3 text-sm"
                    value={targetId}
                    disabled={
                      busy || Boolean(selection?.candidate.librarySkillId)
                    }
                    onChange={(event) => {
                      setTargetId(event.target.value);
                      setConfirmLocal(false);
                    }}
                  >
                    <option value="">
                      {t("skills.external.chooseTarget")}
                    </option>
                    {orderedSkills.map((skill) => (
                      <option key={skill.id} value={skill.id}>
                        {skill.displayName} ({skill.directory})
                        {selection?.candidate.suggestedLibrarySkillIds.includes(
                          skill.id,
                        )
                          ? ` — ${t(skill.id === recommendation?.skill?.id ? "skills.external.recommended" : "skills.external.suggestion")}`
                          : ""}
                      </option>
                    ))}
                  </select>
                  {recommendation?.skill &&
                    targetId === recommendation.skill.id && (
                      <p className="text-sm text-primary" role="status">
                        {t(
                          `skills.external.recommendation.${recommendation.reason}`,
                        )}
                      </p>
                    )}
                  {recommendation &&
                    !recommendation.skill &&
                    recommendation.suggestions.length > 1 && (
                      <p className="text-sm text-muted-foreground">
                        {t("skills.external.recommendation.ambiguous")}
                      </p>
                    )}
                </div>
                {target && (
                  <div className="break-words text-muted-foreground">
                    <p>
                      {t("skills.external.currentSource", {
                        source:
                          target.source.repoOwner && target.source.repoName
                            ? `${target.source.repoOwner}/${target.source.repoName} · ${target.source.skillPath || "."} @ ${target.source.repoBranch || t("skills.external.defaultBranch")}`
                            : t("skills.external.unlinked"),
                      })}
                    </p>
                    {target.description}
                  </div>
                )}
                <p className="text-muted-foreground">
                  {t("skills.external.overwriteHint")}
                </p>
                <label className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    checked={confirmLocal}
                    disabled={busy}
                    onChange={(event) => setConfirmLocal(event.target.checked)}
                  />
                  {t("skills.external.confirmLocal")}
                </label>
              </>
            )}
          </DialogBody>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                if (selection) resetSelection();
                else setOpen(false);
              }}
            >
              {t(selection ? "common.back" : "common.close")}
            </Button>
            {selection && (
              <>
                <Button
                  variant="outline"
                  disabled={
                    busy ||
                    !target ||
                    Boolean(selection.candidate.librarySkillId)
                  }
                  onClick={() => void run(false)}
                >
                  {t("skills.external.linkOnly")}
                </Button>
                <Button
                  disabled={
                    busy ||
                    !target ||
                    (selection.candidate.localModified && !confirmLocal)
                  }
                  onClick={() => void run(true)}
                >
                  {t(
                    selection.candidate.librarySkillId
                      ? "skills.external.update"
                      : "skills.external.linkAndUpdate",
                  )}
                </Button>
              </>
            )}
          </DialogFooter>
        </SkillsDialogContent>
      </Dialog>
    </>
  );
}
