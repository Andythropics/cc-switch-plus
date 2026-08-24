import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, FolderArchive, Link2, Upload } from "lucide-react";

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
import { useApplyGlobalSkillImport } from "@/hooks/useSkills";
import {
  getSkillTechnicalDetails,
  SkillTechnicalDetails,
} from "@/components/skills/SkillTechnicalDetails";
import {
  ProgressiveSkillListFooter,
  useProgressiveSkillList,
} from "@/components/skills/ProgressiveSkillList";
import type {
  GlobalSkillImportFinding,
  GlobalSkillImportInspection,
  GlobalSkillImportMode,
  GlobalSkillImportResolution,
  GlobalSkillImportResult,
} from "@/lib/api/globalSkillImports";

interface GlobalSkillImportPanelProps {
  inspection?: GlobalSkillImportInspection;
  isLoading: boolean;
  isFetching: boolean;
  onRefetch: () => Promise<unknown>;
  onBusyChange?: (busy: boolean) => void;
}

type ResolutionKind = GlobalSkillImportResolution["kind"];

function defaultDirectory(finding: GlobalSkillImportFinding) {
  return (
    finding.directoryCollision.suggestions[0] ?? `${finding.directory}-import`
  );
}

export function GlobalSkillImportPanel({
  inspection,
  isLoading,
  isFetching,
  onRefetch,
  onBusyChange,
}: GlobalSkillImportPanelProps) {
  const { t } = useTranslation();
  const applyImport = useApplyGlobalSkillImport();
  const findings = inspection?.findings ?? [];
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [mode, setMode] = useState<GlobalSkillImportMode>("import_only");
  const [resolutionKind, setResolutionKind] =
    useState<ResolutionKind>("create_new");
  const [directory, setDirectory] = useState("");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [outcomes, setOutcomes] = useState<
    Record<string, GlobalSkillImportResult>
  >({});
  const selected = findings.find((finding) => finding.id === selectedId);
  const progressiveFindings = useProgressiveSkillList(
    findings,
    inspection?.observationToken ?? "empty",
  );

  useEffect(() => {
    onBusyChange?.(applyImport.isPending || confirmOpen);
  }, [applyImport.isPending, confirmOpen, onBusyChange]);

  useEffect(
    () => () => {
      onBusyChange?.(false);
    },
    [onBusyChange],
  );

  useEffect(() => {
    if (selectedId && !findings.some((finding) => finding.id === selectedId)) {
      setSelectedId(null);
    }
  }, [findings, selectedId]);

  const chooseFinding = (finding: GlobalSkillImportFinding) => {
    if (selectedId === finding.id) {
      setSelectedId(null);
      return;
    }
    setSelectedId(finding.id);
    setMode("import_only");
    if (finding.libraryMatch.kind === "identical") {
      setResolutionKind("reuse");
      setDirectory(finding.libraryMatch.directory ?? finding.directory);
    } else {
      setResolutionKind("create_new");
      setDirectory(
        finding.directoryCollision.kind === "none"
          ? finding.directory
          : defaultDirectory(finding),
      );
    }
  };

  const resolution = (): GlobalSkillImportResolution | null => {
    if (!selected) return null;
    const librarySkillId = selected.libraryMatch.librarySkillId;
    if (resolutionKind === "reuse") {
      return librarySkillId ? { kind: "reuse", librarySkillId } : null;
    }
    if (resolutionKind === "replace_library") {
      return librarySkillId
        ? { kind: "replace_library", librarySkillId, confirmed: true }
        : null;
    }
    const value = directory.trim();
    return value ? { kind: "create_new", directory: value } : null;
  };

  const targetDirectory = useMemo(() => {
    if (!selected) return "";
    return resolutionKind === "create_new"
      ? directory.trim()
      : (selected.libraryMatch.directory ?? selected.directory);
  }, [directory, resolutionKind, selected]);
  const validDirectory = /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(directory.trim());
  const canReplace = Boolean(
    selected?.replaceEligibility.eligible &&
      selected.compatibility[selected.consumer].compatible &&
      targetDirectory === selected.directory,
  );
  const canSubmit = Boolean(
    selected &&
      selected.validation.status === "valid" &&
      (resolutionKind !== "create_new" || validDirectory) &&
      (mode === "import_only" || canReplace) &&
      resolution(),
  );

  const execute = async () => {
    const selectedResolution = resolution();
    if (!selected || !inspection || !selectedResolution || !canSubmit) return;
    try {
      const result = await applyImport.mutateAsync({
        findingId: selected.id,
        observationToken: inspection.observationToken,
        mode,
        resolution: selectedResolution,
      });
      setOutcomes((current) => ({ ...current, [selected.id]: result }));
      if (result.outcome === "stale") await onRefetch();
    } catch (error) {
      setOutcomes((current) => ({
        ...current,
        [selected.id]: {
          findingId: selected.id,
          outcome: "blocked",
          message: getSkillTechnicalDetails(error),
        },
      }));
    }
  };

  if (!isLoading && !isFetching && findings.length === 0) return null;

  return (
    <section
      className="space-y-3 rounded-xl border bg-card p-4"
      data-testid="global-skill-imports"
    >
      <div className="flex items-center gap-2">
        <FolderArchive className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-semibold">
          {t("skills.global.import.title")}
        </h3>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("skills.global.import.description")}
      </p>
      {isLoading || isFetching ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.global.import.scanning")}
        </p>
      ) : (
        <div className="space-y-2">
          {progressiveFindings.visibleItems.map((finding) => {
            const outcome = outcomes[finding.id];
            const active = selectedId === finding.id;
            return (
              <div
                key={finding.id}
                className={`rounded-lg border p-3 ${active ? "border-primary" : ""}`}
              >
                <label className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    className="mt-1 h-4 w-4"
                    aria-label={t("skills.global.import.selectFinding", {
                      directory: finding.directory,
                      consumer: finding.consumer,
                    })}
                    checked={active}
                    onChange={() => chooseFinding(finding)}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="min-w-0 break-all font-medium">
                        {finding.directory}
                      </span>
                      <Badge variant="outline">{finding.consumer}</Badge>
                    </span>
                    <span className="mt-1 block break-all font-mono text-xs text-muted-foreground">
                      {finding.sourcePath}
                    </span>
                  </span>
                </label>

                {outcome && (
                  <div className="mt-2 text-xs text-muted-foreground">
                    <Badge
                      variant={
                        [
                          "blocked",
                          "stale",
                          "rolled_back",
                          "recovery_required",
                        ].includes(outcome.outcome)
                          ? "destructive"
                          : "secondary"
                      }
                    >
                      {t(`skills.global.import.outcome.${outcome.outcome}`)}
                    </Badge>{" "}
                    <SkillTechnicalDetails details={outcome.message} />
                    {outcome.backupPath && (
                      <code className="ml-1 break-all">
                        {outcome.backupPath}
                      </code>
                    )}
                  </div>
                )}

                {active && (
                  <div className="mt-3 space-y-3 border-t pt-3">
                    {finding.validation.status === "invalid" && (
                      <p className="text-xs text-destructive">
                        {finding.validation.issues.join("; ")}
                      </p>
                    )}
                    <div className="flex flex-wrap gap-2">
                      <Button
                        size="sm"
                        variant={mode === "import_only" ? "default" : "outline"}
                        onClick={() => setMode("import_only")}
                      >
                        <Upload className="mr-1.5 h-3.5 w-3.5" />
                        {t("skills.global.import.modeImportOnly")}
                      </Button>
                      <Button
                        size="sm"
                        variant={
                          mode === "import_and_replace" ? "default" : "outline"
                        }
                        disabled={!canReplace}
                        onClick={() => setMode("import_and_replace")}
                      >
                        <Link2 className="mr-1.5 h-3.5 w-3.5" />
                        {t("skills.global.import.modeImportAndReplace")}
                      </Button>
                    </div>

                    {finding.libraryMatch.kind !== "identical" && (
                      <div className="space-y-2">
                        <div className="flex flex-wrap gap-3 text-xs">
                          <label className="flex items-center gap-1.5">
                            <input
                              type="radio"
                              checked={resolutionKind === "create_new"}
                              onChange={() => setResolutionKind("create_new")}
                            />
                            {t("skills.global.import.createNew")}
                          </label>
                          {finding.libraryMatch.kind === "different" && (
                            <label className="flex items-center gap-1.5">
                              <input
                                type="radio"
                                checked={resolutionKind === "replace_library"}
                                onChange={() =>
                                  setResolutionKind("replace_library")
                                }
                              />
                              {t("skills.global.import.replaceLibrary")}
                            </label>
                          )}
                        </div>
                        {resolutionKind === "create_new" && (
                          <div>
                            <Label
                              htmlFor={`global-import-directory-${finding.id}`}
                            >
                              {t("skills.global.import.directory")}
                            </Label>
                            <Input
                              id={`global-import-directory-${finding.id}`}
                              value={directory}
                              onChange={(event) =>
                                setDirectory(event.target.value)
                              }
                              className="mt-1"
                            />
                          </div>
                        )}
                      </div>
                    )}

                    <Button
                      size="sm"
                      disabled={!canSubmit || applyImport.isPending}
                      onClick={() => {
                        if (
                          mode === "import_and_replace" ||
                          resolutionKind === "replace_library"
                        ) {
                          setConfirmOpen(true);
                        } else {
                          void execute();
                        }
                      }}
                    >
                      {t("skills.global.import.submit")}
                    </Button>
                  </div>
                )}
              </div>
            );
          })}
          <ProgressiveSkillListFooter
            visibleCount={progressiveFindings.visibleCount}
            totalCount={progressiveFindings.totalCount}
            hasMore={progressiveFindings.hasMore}
            onShowMore={progressiveFindings.showMore}
          />
        </div>
      )}

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("skills.global.import.replaceTitle")}</DialogTitle>
            <DialogDescription>
              {t("skills.global.import.replaceDescription")}
            </DialogDescription>
          </DialogHeader>
          <p className="flex items-start gap-2 text-sm text-destructive">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {t("skills.global.import.installerWarning")}
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              {t("skills.global.import.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={!canSubmit || applyImport.isPending}
              onClick={() => {
                setConfirmOpen(false);
                void execute();
              }}
            >
              {t("skills.global.import.replaceConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
