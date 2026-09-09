import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkillsDialogContent } from "./SkillsDialogContent";
import { useApplyLibrarySkillUpdate } from "@/hooks/useSkills";
import type {
  LibrarySkill,
  LibrarySkillUpdateCheckResult,
  LibrarySkillUpdateResult,
} from "@/lib/api/skills";

interface UpdateItem {
  skill: LibrarySkill;
  check: LibrarySkillUpdateCheckResult;
}

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: UpdateItem[];
  onUpdated: (id: string) => void;
  onRecheck: () => Promise<void>;
}

interface ReviewResult {
  item: UpdateItem;
  snapshot: string;
  result?: LibrarySkillUpdateResult;
  error?: string;
}

const snapshotOf = ({ skill, check }: UpdateItem) =>
  JSON.stringify([skill.id, check]);
const succeeded = (result?: LibrarySkillUpdateResult) =>
  result?.outcome === "updated" || result?.outcome === "up_to_date";
const structurallyReady = ({ skill, check }: UpdateItem) =>
  check.librarySkillId === skill.id &&
  check.outcome === "update_available" &&
  Boolean(check.stageToken) &&
  !check.affectedDeployments.some((impact) => !impact.stagedCompatible);

export function AvailableSkillUpdatesDialog({
  open,
  onOpenChange,
  items,
  onUpdated,
  onRecheck,
}: Props) {
  const { t } = useTranslation();
  const apply = useApplyLibrarySkillUpdate();
  const operation = useRef(false);
  const [busy, setBusy] = useState(false);
  const [activeId, setActiveId] = useState<string>();
  const [consents, setConsents] = useState<Record<string, string>>({});
  const [results, setResults] = useState<Record<string, ReviewResult>>({});
  const [recheckError, setRecheckError] = useState<string>();
  const [recoveryStopped, setRecoveryStopped] = useState(false);
  const pending = busy || apply.isPending;

  const ready = (item: UpdateItem) => {
    const previous = results[item.skill.id];
    return (
      structurallyReady(item) &&
      previous?.result?.outcome !== "recovery_required" &&
      (!previous || previous.snapshot !== snapshotOf(item))
    );
  };
  const confirmed = (item: UpdateItem) =>
    !item.check.localModified || consents[item.skill.id] === snapshotOf(item);
  const readyItems = items.filter(ready);
  const visibleItems = [
    ...items,
    ...Object.values(results)
      .filter(
        ({ item }) => !items.some(({ skill }) => skill.id === item.skill.id),
      )
      .map(({ item }) => item),
  ];

  const changeOpen = (next: boolean) => {
    if (operation.current || apply.isPending) return;
    if (!next) setConsents({});
    onOpenChange(next);
  };

  const run = async (requested: UpdateItem[]) => {
    if (operation.current || apply.isPending || requested.length === 0) return;
    if (!requested.every((item) => ready(item) && confirmed(item))) return;
    // Keep the exact reviewed snapshots and consent throughout the batch.
    // A refreshed prop must never silently substitute a newer staged update.
    operation.current = true;
    setBusy(true);
    setRecoveryStopped(false);
    try {
      for (const item of requested) {
        const { skill, check } = item;
        setActiveId(skill.id);
        try {
          const result = await apply.mutateAsync({
            librarySkillId: skill.id,
            observationToken: check.observationToken,
            stageToken: check.stageToken!,
            confirmLocalModifications: check.localModified,
          });
          setResults((current) => ({
            ...current,
            [skill.id]: { item, snapshot: snapshotOf(item), result },
          }));
          if (succeeded(result)) onUpdated(skill.id);
          if (result.outcome === "recovery_required") {
            setRecoveryStopped(true);
            break;
          }
        } catch (error) {
          // A transport failure cannot prove the stage is still usable.
          setResults((current) => ({
            ...current,
            [skill.id]: {
              item,
              snapshot: snapshotOf(item),
              error: error instanceof Error ? error.message : String(error),
            },
          }));
        }
      }
    } finally {
      operation.current = false;
      setBusy(false);
      setActiveId(undefined);
    }
  };

  const recheck = async () => {
    if (operation.current || apply.isPending) return;
    operation.current = true;
    setBusy(true);
    setRecheckError(undefined);
    setConsents({});
    try {
      await onRecheck();
      // Failed rows unlock only when their actual checked snapshot changes.
    } catch (error) {
      setRecheckError(error instanceof Error ? error.message : String(error));
    } finally {
      operation.current = false;
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <SkillsDialogContent className="max-w-3xl" closeBlocked={pending}>
        <DialogHeader>
          <DialogTitle>{t("skills.library.update.availableTitle")}</DialogTitle>
          <DialogDescription>
            {t("skills.library.update.availableDescription")}
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-3">
          <p className="text-sm text-muted-foreground">
            {t("skills.library.update.backupNotice")}
          </p>
          {visibleItems.length === 0 && (
            <p>{t("skills.library.update.reviewEmpty")}</p>
          )}
          {visibleItems.map((item) => {
            const { skill, check } = item;
            const previous = results[skill.id];
            const completed =
              previous?.snapshot === snapshotOf(item) &&
              succeeded(previous.result);
            const compatibilityBlocked = check.affectedDeployments.some(
              (impact) => !impact.stagedCompatible,
            );
            const needsRecheck =
              !completed &&
              (!check.stageToken ||
                (previous?.snapshot === snapshotOf(item) &&
                  previous.result?.outcome !== "recovery_required"));
            return (
              <article
                key={skill.id}
                aria-label={skill.displayName}
                className="space-y-3 rounded-lg border border-border-default p-4 text-sm"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <h3 className="break-words font-medium">
                      {skill.displayName}
                    </h3>
                    <code className="break-all text-xs text-muted-foreground">
                      {skill.directory}
                    </code>
                  </div>
                  {!completed && (
                    <Button
                      size="sm"
                      disabled={pending || !ready(item) || !confirmed(item)}
                      aria-label={`${t("skills.library.update.updateOne")} ${skill.displayName}`}
                      onClick={() => void run([item])}
                    >
                      {activeId === skill.id && (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      )}
                      {t("skills.library.update.updateOne")}
                    </Button>
                  )}
                </div>
                {check.localModified && !completed && (
                  <label className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-destructive">
                    <input
                      type="checkbox"
                      disabled={pending || !ready(item)}
                      checked={consents[skill.id] === snapshotOf(item)}
                      onChange={(event) =>
                        setConsents((current) => ({
                          ...current,
                          [skill.id]: event.target.checked
                            ? snapshotOf(item)
                            : "",
                        }))
                      }
                    />
                    <span>
                      {t("skills.library.update.confirmLocalModifications")}
                    </span>
                  </label>
                )}
                {compatibilityBlocked && (
                  <p className="text-destructive">
                    {t("skills.library.update.compatibilityRegression")}
                  </p>
                )}
                {check.message && (
                  <p className="break-words">{check.message}</p>
                )}
                {previous &&
                  (previous.snapshot === snapshotOf(item) ||
                    previous.result?.outcome === "recovery_required") && (
                    <div
                      role={succeeded(previous.result) ? "status" : "alert"}
                      className="space-y-1 break-words"
                    >
                      <p>
                        {previous.result
                          ? t(
                              `skills.library.update.applyOutcome.${previous.result.outcome}`,
                            )
                          : t("skills.library.update.applyFailed")}
                      </p>
                      {previous.result?.reason && (
                        <p>
                          {t(
                            `skills.library.update.reason.${previous.result.reason}`,
                          )}
                        </p>
                      )}
                      {(previous.result?.message || previous.error) && (
                        <p>{previous.result?.message || previous.error}</p>
                      )}
                      {previous.result?.backupPath && (
                        <p>
                          {t("skills.library.update.backupPath")}:{" "}
                          <code className="break-all">
                            {previous.result.backupPath}
                          </code>
                        </p>
                      )}
                    </div>
                  )}
                {needsRecheck && (
                  <p>{t("skills.library.update.recheckRequired")}</p>
                )}
              </article>
            );
          })}
          {recoveryStopped && (
            <p role="alert">{t("skills.library.update.batchStopped")}</p>
          )}
          {recheckError && (
            <p role="alert">
              {t("skills.library.update.recheckFailed")}: {recheckError}
            </p>
          )}
          <p className="text-xs text-muted-foreground">
            {t("skills.library.update.batchReadyHint")}
          </p>
        </DialogBody>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={pending}
            onClick={() => changeOpen(false)}
          >
            {t("common.close")}
          </Button>
          <Button
            variant="outline"
            disabled={pending}
            onClick={() => void recheck()}
          >
            {t("skills.library.update.recheck")}
          </Button>
          <Button
            disabled={
              pending || readyItems.length === 0 || !readyItems.every(confirmed)
            }
            onClick={() => void run(readyItems)}
          >
            {pending && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("skills.library.update.updateAll", { count: readyItems.length })}
          </Button>
        </DialogFooter>
      </SkillsDialogContent>
    </Dialog>
  );
}
