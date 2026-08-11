import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertCircle,
  CheckCircle2,
  FolderArchive,
  GitBranch,
  Link2,
  Upload,
} from "lucide-react";

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
  useApplyProjectSkillImport,
  useInspectProjectSkillImports,
} from "@/hooks/useSkills";
import type {
  ProjectSkillImportFinding,
  ProjectSkillImportIntent,
  ProjectSkillImportMode,
  ProjectSkillImportOutcome,
  ProjectSkillImportResult,
  ProjectSkillImportReplaceBlockReason,
  ProjectSkillImportResolution,
} from "@/lib/api/projectWorkspaces";

interface ProjectSkillImportPanelProps {
  workspaceId: string;
}

type ResolutionKind = ProjectSkillImportResolution["kind"];
const importConsumers = ["claude", "codex"] as const;

const replacementReasonKeys: Record<
  ProjectSkillImportReplaceBlockReason,
  string
> = {
  directory_identity_mismatch:
    "skills.projects.import.directoryIdentityMismatch",
  git_tracked_content: "skills.projects.import.trackedBlocker",
  nested_unsupported: "skills.projects.import.nestedUnsupported",
  invalid_source: "skills.projects.import.invalidSource",
};

const outcomeKeys: Record<ProjectSkillImportOutcome, string> = {
  reused: "skills.projects.import.outcome.reused",
  created: "skills.projects.import.outcome.created",
  library_replaced: "skills.projects.import.outcome.library_replaced",
  deployed: "skills.projects.import.outcome.deployed",
  blocked: "skills.projects.import.outcome.blocked",
  stale: "skills.projects.import.outcome.stale",
  rolled_back: "skills.projects.import.outcome.rolled_back",
  recovery_required: "skills.projects.import.outcome.recovery_required",
};

function defaultDirectory(finding: ProjectSkillImportFinding) {
  return (
    finding.directoryCollision.suggestions[0] ??
    (finding.directoryCollision.kind === "none"
      ? finding.directory
      : `${finding.directory}-import`)
  );
}

export function ProjectSkillImportPanel({
  workspaceId,
}: ProjectSkillImportPanelProps) {
  const { t } = useTranslation();
  const inspectionQuery = useInspectProjectSkillImports(workspaceId);
  const applyImport = useApplyProjectSkillImport();
  const findings = inspectionQuery.data?.findings ?? [];
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [mode, setMode] = useState<ProjectSkillImportMode>("import_only");
  const [resolutionKind, setResolutionKind] =
    useState<ResolutionKind>("create_new");
  const [directory, setDirectory] = useState("");
  const [replaceDialogOpen, setReplaceDialogOpen] = useState(false);
  const [outcomes, setOutcomes] = useState<
    Record<string, ProjectSkillImportResult>
  >({});

  const selected = findings.find((finding) => finding.id === selectedId);

  useEffect(() => {
    if (selectedId && !findings.some((finding) => finding.id === selectedId)) {
      setSelectedId(null);
    }
  }, [findings, selectedId]);

  const chooseFinding = (finding: ProjectSkillImportFinding) => {
    if (finding.scope !== "root_level") return;
    if (selectedId === finding.id) {
      setSelectedId(null);
      return;
    }
    setSelectedId(finding.id);
    setMode("import_only");
    if (
      finding.libraryMatch.kind === "identical" &&
      finding.directoryCollision.kind === "none"
    ) {
      setResolutionKind("reuse");
      setDirectory(finding.libraryMatch.directory ?? finding.directory);
    } else {
      setResolutionKind("create_new");
      setDirectory(defaultDirectory(finding));
    }
  };

  const identityMismatch = useMemo(() => {
    if (!selected) return false;
    const targetDirectory =
      resolutionKind === "create_new"
        ? directory.trim()
        : (selected.libraryMatch.directory ?? selected.directory);
    return Boolean(targetDirectory && targetDirectory !== selected.directory);
  }, [directory, resolutionKind, selected]);

  const selectedConsumerCompatibility = selected
    ? selected.compatibility[selected.consumer]
    : undefined;
  const replacementBlockReason = useMemo<
    ProjectSkillImportReplaceBlockReason | undefined
  >(() => {
    if (!selected) return undefined;
    if (identityMismatch) return "directory_identity_mismatch";
    if (!selected.replaceEligibility.eligible) {
      return selected.replaceEligibility.reason ?? "invalid_source";
    }
    if (selected.git.tracked) return "git_tracked_content";
    if (selected.scope !== "root_level") return "nested_unsupported";
    if (selected.validation.status !== "valid") return "invalid_source";
    if (!selectedConsumerCompatibility?.compatible) return "invalid_source";
    return undefined;
  }, [identityMismatch, selected, selectedConsumerCompatibility]);

  const readableDirectory = (value: string) =>
    /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value.trim());
  const invalidDirectory =
    resolutionKind === "create_new" &&
    Boolean(directory.trim()) &&
    (!readableDirectory(directory) ||
      (selected &&
        ((selected.directoryCollision.kind !== "none" &&
          directory.trim() === selected.directoryCollision.requested) ||
          (selected.libraryMatch.kind === "different" &&
            directory.trim() === selected.directory))));
  const canImport = Boolean(
    selected &&
      selected.validation.status === "valid" &&
      (mode !== "import_and_replace" ||
        selectedConsumerCompatibility?.compatible) &&
      (resolutionKind !== "create_new" ||
        (directory.trim() && !invalidDirectory)) &&
      (mode === "import_only" || !replacementBlockReason),
  );

  const resolution = (): ProjectSkillImportResolution | null => {
    if (!selected) return null;
    if (resolutionKind === "reuse") {
      const librarySkillId = selected.libraryMatch.librarySkillId;
      return librarySkillId ? { kind: "reuse", librarySkillId } : null;
    }
    if (resolutionKind === "create_new") {
      const value = directory.trim();
      return value ? { kind: "create_new", directory: value } : null;
    }
    const librarySkillId = selected.libraryMatch.librarySkillId;
    return librarySkillId
      ? { kind: "replace_library", librarySkillId, confirmed: true }
      : null;
  };

  const executeImport = async () => {
    if (!selected || !inspectionQuery.data || !canImport) return;
    const selectedResolution = resolution();
    if (!selectedResolution) return;
    const intent: ProjectSkillImportIntent = {
      workspaceId,
      findingId: selected.id,
      observationToken: inspectionQuery.data.observationToken,
      mode,
      resolution: selectedResolution,
    };
    try {
      const result = await applyImport.mutateAsync(intent);
      setOutcomes((current) => ({ ...current, [selected.id]: result }));
      if (result.outcome === "stale") {
        void inspectionQuery.refetch();
      }
    } catch (error) {
      setOutcomes((current) => ({
        ...current,
        [selected.id]: {
          findingId: selected.id,
          outcome: "blocked",
          message:
            error instanceof Error
              ? error.message
              : t("skills.projects.import.invalidSource"),
        },
      }));
    }
  };

  const submitImport = () => {
    if (!selected || !canImport) return;
    const needsConfirmation =
      mode === "import_and_replace" || resolutionKind === "replace_library";
    if (needsConfirmation) {
      setReplaceDialogOpen(true);
      return;
    }
    void executeImport();
  };

  return (
    <section
      className="space-y-3 border-t pt-3"
      data-testid="project-skill-imports"
    >
      <div className="flex items-center gap-2">
        <FolderArchive className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-semibold">
          {t("skills.projects.import.title")}
        </h3>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("skills.projects.import.description")}
      </p>

      {inspectionQuery.isLoading || inspectionQuery.isFetching ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.projects.import.scanning")}
        </p>
      ) : findings.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("skills.projects.import.empty")}
        </p>
      ) : (
        <div className="space-y-2">
          {findings.map((finding) => {
            const supported = finding.scope === "root_level";
            const outcome = outcomes[finding.id];
            return (
              <div
                key={finding.id}
                className={`rounded-lg border p-3 ${selectedId === finding.id ? "border-primary" : ""}`}
              >
                <div className="flex items-start gap-2">
                  {supported ? (
                    <input
                      type="checkbox"
                      aria-label={t("skills.projects.import.selectFinding", {
                        directory: finding.directory,
                      })}
                      checked={selectedId === finding.id}
                      onChange={() => chooseFinding(finding)}
                      className="mt-1 h-4 w-4"
                    />
                  ) : (
                    <AlertCircle className="mt-0.5 h-4 w-4 text-muted-foreground" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium">{finding.directory}</span>
                      <Badge variant="outline">{finding.consumer}</Badge>
                      {supported ? (
                        <Badge variant="secondary">
                          {t("skills.projects.import.rootLevel")}
                        </Badge>
                      ) : (
                        <Badge variant="outline">
                          {t("skills.projects.import.nestedUnsupported")}
                        </Badge>
                      )}
                    </div>
                    <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                      {finding.sourcePath}
                    </p>
                    {!supported && (
                      <p className="mt-1 text-xs text-muted-foreground">
                        {t("skills.projects.import.nestedDescription")}
                      </p>
                    )}
                    {outcome && (
                      <div
                        className={`mt-2 flex flex-wrap items-center gap-1 text-xs ${
                          outcome.outcome === "recovery_required"
                            ? "rounded border border-destructive bg-destructive/10 p-2 text-destructive"
                            : ""
                        }`}
                      >
                        <Badge
                          variant={
                            outcome.outcome === "blocked" ||
                            outcome.outcome === "stale" ||
                            outcome.outcome === "rolled_back" ||
                            outcome.outcome === "recovery_required"
                              ? "destructive"
                              : "secondary"
                          }
                        >
                          {t(outcomeKeys[outcome.outcome])}
                        </Badge>
                        {outcome.reason && (
                          <span className="text-muted-foreground">
                            {t(
                              `skills.projects.import.reason.${outcome.reason}`,
                            )}
                          </span>
                        )}
                        {outcome.message && (
                          <span className="text-muted-foreground">
                            {outcome.message}
                          </span>
                        )}
                        {outcome.outcome === "recovery_required" && (
                          <span className="font-semibold">
                            {t("skills.projects.import.recoveryRequired")}
                          </span>
                        )}
                        {outcome.backupPath && (
                          <span className="w-full font-mono">
                            <span>
                              {t("skills.projects.import.backupPath")}:
                            </span>{" "}
                            <code>{outcome.backupPath}</code>
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                </div>

                {selectedId === finding.id && supported && (
                  <div className="mt-3 space-y-3 border-t pt-3">
                    <div className="grid gap-2 text-xs sm:grid-cols-2">
                      <div className="flex items-center gap-1.5">
                        {finding.validation.status === "valid" ? (
                          <CheckCircle2 className="h-3.5 w-3.5 text-green-600" />
                        ) : (
                          <AlertCircle className="h-3.5 w-3.5 text-destructive" />
                        )}
                        <span>
                          {t(
                            finding.validation.status === "valid"
                              ? "skills.projects.import.validationValid"
                              : "skills.projects.import.validationInvalid",
                          )}
                        </span>
                      </div>
                      {importConsumers.map((consumer) => (
                        <div
                          className="flex items-center gap-1.5"
                          key={consumer}
                        >
                          <Link2 className="h-3.5 w-3.5" />
                          <span>
                            {t("skills.projects.import.compatibility", {
                              consumer,
                            })}
                            {finding.compatibility[consumer].compatible
                              ? " ✓"
                              : " ✕"}
                          </span>
                        </div>
                      ))}
                    </div>

                    {finding.validation.issues.length > 0 && (
                      <ul className="list-disc pl-5 text-xs text-destructive">
                        {finding.validation.issues.map((issue) => (
                          <li key={issue}>{issue}</li>
                        ))}
                      </ul>
                    )}
                    {finding.libraryMatch.kind === "identical" && (
                      <p className="text-xs text-muted-foreground">
                        {t("skills.projects.import.libraryReuse")}
                      </p>
                    )}
                    {finding.libraryMatch.kind === "different" && (
                      <p className="text-xs text-muted-foreground">
                        {t("skills.projects.import.libraryDifferent")}
                      </p>
                    )}
                    {finding.git.tracked && (
                      <p className="flex items-start gap-1.5 text-xs text-destructive">
                        <GitBranch className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <span>
                          <span>
                            {t("skills.projects.import.trackedBlocker")}
                          </span>{" "}
                          <span>
                            {t("skills.projects.import.trackedNoMutation")}
                          </span>
                        </span>
                      </p>
                    )}
                    {finding.directoryCollision.kind !== "none" && (
                      <p className="text-xs text-destructive">
                        <span>
                          {t("skills.projects.import.directoryCollision")}
                        </span>
                        {finding.directoryCollision.suggestions.length > 0 && (
                          <span>
                            :{" "}
                            {finding.directoryCollision.suggestions.join(", ")}
                          </span>
                        )}
                      </p>
                    )}
                    <p className="text-xs text-muted-foreground">
                      {t("skills.projects.import.localCopyDescription")}
                    </p>

                    <div className="flex flex-wrap gap-2">
                      <Button
                        type="button"
                        size="sm"
                        variant={mode === "import_only" ? "default" : "outline"}
                        onClick={() => setMode("import_only")}
                      >
                        <Upload className="mr-1.5 h-3.5 w-3.5" />
                        {t("skills.projects.import.modeImportOnly")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant={
                          mode === "import_and_replace" ? "default" : "outline"
                        }
                        disabled={
                          Boolean(replacementBlockReason) ||
                          !selectedConsumerCompatibility?.compatible
                        }
                        title={
                          replacementBlockReason
                            ? t(replacementReasonKeys[replacementBlockReason])
                            : undefined
                        }
                        onClick={() => setMode("import_and_replace")}
                      >
                        <Link2 className="mr-1.5 h-3.5 w-3.5" />
                        {t("skills.projects.import.modeImportAndReplace")}
                      </Button>
                    </div>

                    {(finding.libraryMatch.kind !== "identical" ||
                      finding.directoryCollision.kind !== "none") && (
                      <div className="space-y-2">
                        <div className="flex flex-wrap gap-3 text-xs">
                          <label className="flex items-center gap-1.5">
                            <input
                              type="radio"
                              name={`resolution-${finding.id}`}
                              checked={resolutionKind === "create_new"}
                              onChange={() => setResolutionKind("create_new")}
                            />
                            {t("skills.projects.import.createNew")}
                          </label>
                          {finding.libraryMatch.kind === "different" &&
                            finding.libraryMatch.librarySkillId && (
                              <label className="flex items-center gap-1.5">
                                <input
                                  type="radio"
                                  name={`resolution-${finding.id}`}
                                  checked={resolutionKind === "replace_library"}
                                  onChange={() =>
                                    setResolutionKind("replace_library")
                                  }
                                />
                                {t("skills.projects.import.replaceLibrary")}
                              </label>
                            )}
                        </div>
                        {resolutionKind === "create_new" && (
                          <div>
                            <Label htmlFor={`import-directory-${finding.id}`}>
                              {t("skills.projects.import.directory")}
                            </Label>
                            <Input
                              id={`import-directory-${finding.id}`}
                              aria-label={t("skills.projects.import.directory")}
                              value={directory}
                              onChange={(event) =>
                                setDirectory(event.target.value)
                              }
                              className="mt-1"
                            />
                            {invalidDirectory && (
                              <p className="mt-1 text-xs text-destructive">
                                {t("skills.projects.import.invalidDirectory")}
                              </p>
                            )}
                          </div>
                        )}
                      </div>
                    )}

                    {replacementBlockReason && (
                      <p className="text-xs text-destructive">
                        {t(replacementReasonKeys[replacementBlockReason])}
                      </p>
                    )}
                    {resolutionKind === "replace_library" && (
                      <p className="text-xs text-muted-foreground">
                        {t("skills.projects.import.replaceLibraryDescription")}
                      </p>
                    )}
                    <Button
                      type="button"
                      size="sm"
                      disabled={!canImport || applyImport.isPending}
                      onClick={submitImport}
                    >
                      {t("skills.projects.import.submit")}
                    </Button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      <Dialog open={replaceDialogOpen} onOpenChange={setReplaceDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t("skills.projects.import.replaceTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("skills.projects.import.replaceDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setReplaceDialogOpen(false)}
            >
              {t("skills.projects.import.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={!canImport || applyImport.isPending}
              onClick={() => {
                setReplaceDialogOpen(false);
                void executeImport();
              }}
            >
              {t("skills.projects.import.replaceConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
