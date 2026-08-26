import { invoke } from "@tauri-apps/api/core";

export type AppType =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "pi";

export type LibrarySourceKind = "git" | "zip" | "marketplace" | "local_import";
export type RemoteLibrarySourceKind = Exclude<
  LibrarySourceKind,
  "zip" | "local_import"
>;

export type SkillActivityOperation =
  | "library"
  | "workspace"
  | "deployment"
  | "repair"
  | "migration"
  | "forget"
  | "removal";

export type SkillActivityOutcome =
  | "success"
  | "no_op"
  | "blocked"
  | "conflict"
  | "failed"
  | "compensation_failed"
  | "rolled_back";

export type SkillActivityReason =
  | "acquire"
  | "import"
  | "metadata_update"
  | "update"
  | "register"
  | "rename"
  | "archive"
  | "restore"
  | "relocate"
  | "lifecycle_refresh"
  | "deploy"
  | "replace_foreign_link"
  | "undeploy"
  | "repair"
  | "migrate"
  | "migrate_item"
  | "resume"
  | "deployment_forget"
  | "workspace_forget"
  | "library_remove"
  | "deployment_remove"
  | "legacy_link_remove"
  | "compensation_restore"
  | "import_and_replace"
  | "recover_deployment";

export type SkillActivityDetailCode =
  | "none"
  | "already_in_sync"
  | "already_absent"
  | "stale_observation"
  | "drift"
  | "missing_library"
  | "archived_workspace"
  | "unavailable_workspace"
  | "target_conflict"
  | "invalid_input"
  | "unsupported_platform"
  | "validation_failure"
  | "filesystem_failure"
  | "database_failure"
  | "compensation_failure"
  | "duplicate_key"
  | "partial_batch";

export type SkillActivityActor = "user" | "system" | "migration";
export type SkillActivityTrigger =
  | "command"
  | "startup"
  | "focus"
  | "manual"
  | "batch"
  | "resume";

export interface SkillActivityTarget {
  librarySkillId?: string;
  workspaceId?: string;
  deploymentId?: string;
  consumer?: DeploymentConsumer;
  workspaceKind?: WorkspaceKind;
}

/** Skill 应用启用状态 */
export interface SkillApps {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild?: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
  pi: boolean;
}

export interface SkillActivityBatch {
  batchId: string;
  itemIndex: number;
  itemCount: number;
}

export interface SkillActivityEntry {
  id: number;
  occurredAt: number;
  operation: SkillActivityOperation;
  reason: SkillActivityReason;
  detailCode: SkillActivityDetailCode;
  outcome: SkillActivityOutcome;
  actor: SkillActivityActor;
  trigger: SkillActivityTrigger;
  target?: SkillActivityTarget | null;
  batch?: SkillActivityBatch | null;
}

export interface SkillActivityCursor {
  occurredAt: number;
  id: number;
}

export interface SkillActivityQuery {
  operation?: SkillActivityOperation;
  reason?: SkillActivityReason;
  outcome?: SkillActivityOutcome;
  librarySkillId?: string;
  workspaceId?: string;
  deploymentId?: string;
  consumer?: DeploymentConsumer;
  workspaceKind?: WorkspaceKind;
  since?: number;
  until?: number;
  cursor?: SkillActivityCursor;
  limit?: number;
}

export interface SkillActivityPage {
  entries: SkillActivityEntry[];
  nextCursor?: SkillActivityCursor | null;
  hasMore: boolean;
}

export interface LibrarySkillSource {
  kind: LibrarySourceKind;
  url?: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  skillPath?: string;
  marketplace?: string;
}

export interface ConsumerCompatibility {
  compatible: boolean;
  issues: string[];
}

export interface LibrarySkillCompatibility {
  claude: ConsumerCompatibility;
  codex: ConsumerCompatibility;
}

/** A deployment whose compatibility may regress when a staged snapshot lands. */
export interface LibrarySkillUpdateAffectedDeployment {
  inspection: DeploymentInspection;
  currentCompatible: boolean;
  stagedCompatible: boolean;
}

export type LibrarySkillUpdateCheckOutcome =
  | "update_available"
  | "up_to_date"
  | "not_updatable"
  | "invalid_candidate";

export interface LibrarySkillUpdateCheckResult {
  librarySkillId: string;
  outcome: LibrarySkillUpdateCheckOutcome;
  observationToken: string;
  stageToken?: string;
  recordedContentHash: string;
  liveContentHash?: string;
  stagedContentHash?: string;
  localModified: boolean;
  compatibility?: LibrarySkillCompatibility;
  affectedDeployments: LibrarySkillUpdateAffectedDeployment[];
  message?: string;
}

export type LibrarySkillUpdateApplyOutcome =
  | "updated"
  | "up_to_date"
  | "blocked"
  | "stale"
  | "rolled_back"
  | "recovery_required";

export type LibrarySkillUpdateApplyReason =
  | "local_modification_confirmation_required"
  | "compatibility_regression"
  | "not_updatable"
  | "invalid_candidate"
  | "duplicate_content"
  | "missing_stage"
  | "stale_observation"
  | "compensation_failed";

export interface LibrarySkillUpdateIntent {
  librarySkillId: string;
  observationToken: string;
  stageToken: string;
  confirmLocalModifications: boolean;
}

export interface LibrarySkillUpdateResult {
  librarySkillId: string;
  outcome: LibrarySkillUpdateApplyOutcome;
  reason?: LibrarySkillUpdateApplyReason;
  recordedContentHash?: string;
  liveContentHash?: string;
  stagedContentHash?: string;
  backupPath?: string;
  affectedDeployments: LibrarySkillUpdateAffectedDeployment[];
  message?: string;
}

export type LibrarySkillDeletionAction = "remove_expected_link" | "forget";

export interface LibrarySkillDeletionTarget {
  inspection: DeploymentInspection;
  actionRequired: LibrarySkillDeletionAction;
}

export interface LibrarySkillDeletionInspection {
  librarySkillId: string;
  observationToken: string;
  targets: LibrarySkillDeletionTarget[];
  blocked: boolean;
  message?: string;
}

export interface LibrarySkillDeletionIntent {
  librarySkillId: string;
  observationToken: string;
}

export type LibrarySkillDeletionOutcome =
  | "deleted"
  | "blocked"
  | "stale"
  | "rolled_back"
  | "recovery_required";

export interface LibrarySkillDeletionResult {
  librarySkillId: string;
  outcome: LibrarySkillDeletionOutcome;
  items: DeploymentItemResult[];
  backupPath?: string;
  message?: string;
}

/** Private, undeployed Skill snapshot owned by the Library. */
export interface LibrarySkill {
  id: string;
  /** Immutable direct-child directory identity. */
  directory: string;
  /** User-editable presentation metadata. */
  displayName: string;
  description?: string;
  source: LibrarySkillSource;
  compatibility: LibrarySkillCompatibility;
  contentHash: string;
  acquiredAt: number;
  updatedAt: number;
}

export type DeploymentConsumer = "claude" | "codex";
export type WorkspaceKind = "global" | "project";

export interface DeploymentTarget {
  consumer: DeploymentConsumer;
  workspace: WorkspaceKind;
  workspaceId?: string;
}

export interface SkillUninstallResult {
  backupPath?: string;
  preservedPiPath?: string;
  piCleanupIncomplete?: boolean;
}

export interface DesiredDeployment {
  id: string;
  librarySkillId: string;
  libraryDirectory: string;
  target: DeploymentTarget;
  createdAt: number;
  updatedAt: number;
}

export type ObservedDeploymentState =
  | "missing"
  | "correct_link"
  | "redirected_link"
  | "broken_link"
  | "invalid_link"
  | "occupied_directory"
  | "occupied_file"
  | "unreadable"
  | "library_missing"
  | "unrecorded_link"
  | "invalid_target_root"
  | "unsupported_platform";

export interface ObservedDeployment {
  state: ObservedDeploymentState;
  targetPath: string;
  expectedTarget: string;
  actualTarget?: string;
}

export type DeploymentStatus =
  | "not_deployed"
  | "in_sync"
  | "drift"
  | "conflict"
  | "orphaned"
  | "blocked"
  | "archived"
  | "unsupported";

export interface DeploymentInspection {
  librarySkillId: string;
  libraryDirectory: string;
  target: DeploymentTarget;
  desired?: DesiredDeployment;
  observed: ObservedDeployment;
  /** Hash of the inspection facts required for safe resolution mutations. */
  observationToken: string;
  status: DeploymentStatus;
}

export interface DeploymentQuery {
  consumer?: DeploymentConsumer;
  workspace?: WorkspaceKind;
  workspaceId?: string;
  librarySkillIds?: string[];
}

export interface DeploymentInspectionResult {
  items: DeploymentInspection[];
}

export interface DeploymentRecoveryQuery {
  consumer?: DeploymentConsumer;
  workspace?: WorkspaceKind;
  workspaceId?: string;
}

export type DeploymentRecoveryDisposition =
  | "recoverable"
  | "desired_exists"
  | "foreign_link"
  | "ambiguous_link"
  | "broken_link"
  | "escaping_link"
  | "non_library"
  | "occupied"
  | "unreadable"
  | "invalid_root"
  | "incompatible"
  | "archived_workspace"
  | "unavailable_workspace";

export type DeploymentRecoverySafeReason = "exact_library_link";

/** A read-only observation. Display paths are never accepted back as intent. */
export interface DeploymentRecoveryFinding {
  disposition: DeploymentRecoveryDisposition;
  target: DeploymentTarget;
  entryName: string;
  librarySkillId?: string;
  libraryDirectory?: string;
  observedTarget?: string;
  observationToken?: string;
  safeReason?: DeploymentRecoverySafeReason;
}

export interface DeploymentRecoveryInspectionResult {
  findings: DeploymentRecoveryFinding[];
}

export type SkillsMigrationPreflightStatus =
  | "not_required"
  | "decision_needed"
  | "blocked";

export type SkillsMigrationPageMode = "writable" | "read_only";

export type SkillsMigrationInventoryKind =
  | "managed_library"
  | "legacy_skill"
  | "unmanaged_content"
  | "target_conflict"
  | "legacy_codex_entry"
  | "scan_error";

export type SkillsMigrationInventoryState =
  | "present"
  | "missing"
  | "real_directory"
  | "managed_link"
  | "foreign_link"
  | "broken_link"
  | "occupied"
  | "unreadable"
  | "invalid";

/** Read-only legacy fact. Locations are display output and never intent input. */
export interface SkillsMigrationInventoryItem {
  kind: SkillsMigrationInventoryKind;
  directory?: string;
  consumer?: DeploymentConsumer;
  location: string;
  state: SkillsMigrationInventoryState;
  managedSkillId?: string;
  enabled?: boolean;
}

export type SkillsMigrationPlanDisposition =
  | "perform"
  | "preserve"
  | "preserve_with_consent"
  | "user_resolve";

export type SkillsMigrationPlanAction =
  | "move_to_library"
  | "reuse_library"
  | "create_global_deployment"
  | "remove_legacy_codex_link"
  | "preserve_content"
  | "preserve_unsupported_consumer_files"
  | "resolve_conflict"
  | "repair_preflight"
  | "finalize";

export type SkillsMigrationPlanReason =
  | "proven_managed"
  | "already_in_library"
  | "legacy_enabled"
  | "proven_cc_switch_link"
  | "unmanaged"
  | "unsupported_consumer_enabled"
  | "foreign_or_ambiguous"
  | "content_conflict"
  | "missing_source"
  | "invalid_legacy_state"
  | "unreadable"
  | "migration_finalized";

/** Deterministically ordered proposed work; all locations are display-only. */
export interface SkillsMigrationPlanItem {
  disposition: SkillsMigrationPlanDisposition;
  action: SkillsMigrationPlanAction;
  directory?: string;
  consumer?: DeploymentConsumer;
  fromLocation?: string;
  toLocation?: string;
  reason: SkillsMigrationPlanReason;
  unsupportedConsumers?: Array<"gemini" | "grokbuild" | "opencode" | "hermes">;
}

export interface SkillsMigrationBackupPlan {
  required: boolean;
  ready: boolean;
  recoveryAvailable: boolean;
  databasePath?: string;
  contentPaths: string[];
}

/** Stable, read-only guided migration review. Issue #14 has no apply seam. */
export interface SkillsMigrationPreflight {
  status: SkillsMigrationPreflightStatus;
  observationToken: string;
  pageMode: SkillsMigrationPageMode;
  inventory: SkillsMigrationInventoryItem[];
  plan: SkillsMigrationPlanItem[];
  backup: SkillsMigrationBackupPlan;
  execution?: SkillsMigrationExecutionResult;
}

export type SkillsMigrationReportState =
  | "prepared"
  | "running"
  | "blocked"
  | "recovery_required"
  | "completed"
  | "restored";

/** Durable summary of the most recent guided migration run. */
export interface SkillsMigrationReportSummary {
  performed: number;
  preserved: number;
  open: number;
}

/** A persisted migration follow-up item; paths are display-only. */
export interface SkillsMigrationFinding {
  findingId: string;
  disposition: SkillsMigrationPlanDisposition;
  action?: SkillsMigrationPlanAction;
  directory?: string;
  consumer?: DeploymentConsumer;
  fromLocation?: string;
  toLocation?: string;
  reason?: SkillsMigrationPlanReason;
  unsupportedConsumers?: Array<"gemini" | "grokbuild" | "opencode" | "hermes">;
  status: string;
  origin?: "preflight" | "legacy_backfill";
  detailComplete?: boolean;
}

/** Durable report kept available after the active migration gate disappears. */
export interface SkillsMigrationReport {
  runId: string;
  state: SkillsMigrationReportState;
  createdAt: number;
  completedAt?: number;
  acknowledgedAt?: number;
  observationToken: string;
  summary: SkillsMigrationReportSummary;
  findings: SkillsMigrationFinding[];
  backup?: SkillsMigrationBackup;
}

export interface SkillsMigrationIntent {
  observationToken: string;
  preserveUnsupportedConsumerFiles: boolean;
}

export interface SkillsMigrationRevealIntent {
  observationToken: string;
  planIndex: number;
}

export type SkillsMigrationExecutionOutcome =
  | "completed"
  | "stale_observation"
  | "resumable"
  | "blocked"
  | "recovery_required"
  | "restored";

export interface SkillsMigrationProgress {
  completedItems: number;
  totalItems: number;
}

export interface SkillsMigrationBackup {
  backupId: string;
  createdAt: number;
  restoreAvailable: boolean;
}

export type SkillsMigrationItemOutcome =
  | "completed"
  | "already_completed"
  | "preserved"
  | "rolled_back"
  | "blocked"
  | "recovery_required";

/** Backend-ordered execution fact; locations are display-only. */
export interface SkillsMigrationItemResult {
  action: SkillsMigrationPlanAction;
  outcome: SkillsMigrationItemOutcome;
  directory?: string;
  consumer?: DeploymentConsumer;
  reason?: SkillsMigrationPlanReason;
}

export interface SkillsMigrationExecutionResult {
  outcome: SkillsMigrationExecutionOutcome;
  pageMode: SkillsMigrationPageMode;
  progress: SkillsMigrationProgress;
  items: SkillsMigrationItemResult[];
  backup?: SkillsMigrationBackup;
}

export type DeploymentIntent =
  | { action: "deploy"; librarySkillId: string; target: DeploymentTarget }
  | { action: "undeploy"; librarySkillId: string; target: DeploymentTarget }
  | {
      action: "repair";
      librarySkillId: string;
      target: DeploymentTarget;
      observationToken: string;
    }
  | {
      action: "replaceForeignLink";
      librarySkillId: string;
      target: DeploymentTarget;
      observationToken: string;
      confirmed: true;
    }
  | {
      action: "recover";
      librarySkillId: string;
      target: DeploymentTarget;
      observationToken: string;
      confirmed: true;
    }
  | { action: "forget"; librarySkillId: string; target: DeploymentTarget };

export interface DeploymentBatch {
  intents: DeploymentIntent[];
}

export type DeploymentMutationOutcome =
  | "applied"
  | "replaced"
  | "already_in_sync"
  | "removed"
  | "already_absent"
  | "conflict"
  | "drift"
  | "blocked"
  | "stale_observation"
  | "forgotten"
  | "recovery_required"
  | "error";

/** Typed outcome labels and policies shared by every Skills deployment view. */
export const deploymentOutcomeLabelKeys: Record<
  DeploymentMutationOutcome,
  string
> = {
  applied: "skills.batch.outcome.applied",
  replaced: "skills.batch.outcome.replaced",
  already_in_sync: "skills.batch.outcome.already_in_sync",
  removed: "skills.batch.outcome.removed",
  already_absent: "skills.batch.outcome.already_absent",
  conflict: "skills.batch.outcome.conflict",
  drift: "skills.batch.outcome.drift",
  blocked: "skills.batch.outcome.blocked",
  stale_observation: "skills.batch.outcome.stale_observation",
  forgotten: "skills.batch.outcome.forgotten",
  recovery_required: "skills.batch.outcome.recovery_required",
  error: "skills.batch.outcome.error",
};

export const successfulDeploymentOutcomes: ReadonlySet<DeploymentMutationOutcome> =
  new Set([
    "applied",
    "replaced",
    "already_in_sync",
    "removed",
    "already_absent",
    "forgotten",
  ]);

export const highVisibilityDeploymentOutcomes: ReadonlySet<DeploymentMutationOutcome> =
  new Set([
    "conflict",
    "drift",
    "blocked",
    "stale_observation",
    "recovery_required",
    "error",
  ]);

export interface DeploymentItemResult {
  librarySkillId: string;
  target: DeploymentTarget;
  outcome: DeploymentMutationOutcome;
  message?: string;
  inspection?: DeploymentInspection;
}

export interface DeploymentBatchResult {
  items: DeploymentItemResult[];
}

/** 可发现的 Skill（来自仓库） */
export interface DiscoverableSkill {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
}

/** skills.sh 可发现的技能 */
export interface SkillsShDiscoverableSkill {
  key: string;
  name: string;
  directory: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
  installs: number;
  readmeUrl?: string;
}

/** skills.sh 搜索结果 */
export interface SkillsShSearchResult {
  skills: SkillsShDiscoverableSkill[];
  totalCount: number;
  query: string;
}

/** 仓库配置 */
export interface SkillRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}

// ========== API ==========

export const skillsApi = {
  /** List snapshots in the private Library (never consumer deployments). */
  async getLibrary(): Promise<LibrarySkill[]> {
    return await invoke("getLibrarySkills");
  },

  /** List redacted, device-local Skills activity in newest-first order. */
  async listActivity(query?: SkillActivityQuery): Promise<SkillActivityPage> {
    return await invoke("listSkillActivity", { query: query ?? null });
  },

  /** Inspect desired and observed Claude/Codex deployment state. */
  async inspectDeployments(
    query?: DeploymentQuery,
  ): Promise<DeploymentInspectionResult> {
    return await invoke("inspectSkillDeployments", {
      query: query ?? null,
    });
  },

  /** Find unrecorded Library links without mutating desired or observed state. */
  async inspectDeploymentRecovery(
    query?: DeploymentRecoveryQuery,
  ): Promise<DeploymentRecoveryInspectionResult> {
    return await invoke("inspectDeploymentRecovery", {
      query: query ?? null,
    });
  },

  /** Preview the macOS guided migration without changing legacy state. */
  async inspectSkillsMigrationPreflight(): Promise<SkillsMigrationPreflight> {
    return await invoke("inspectSkillsMigrationPreflight");
  },

  /** Return the latest durable migration report, including completed runs. */
  async inspectLatestSkillsMigrationReport(): Promise<SkillsMigrationReport | null> {
    return await invoke("inspectLatestSkillsMigrationReport");
  },

  /** Mark one durable migration report as acknowledged by the user. */
  async acknowledgeSkillsMigrationReport(
    runId: string,
  ): Promise<SkillsMigrationReport> {
    return await invoke("acknowledgeSkillsMigrationReport", { runId });
  },

  /** Reveal a persisted migration finding selected by its opaque identity. */
  async revealSkillsMigrationFinding(findingId: string): Promise<boolean> {
    return await invoke("revealSkillsMigrationFinding", { findingId });
  },

  /** Execute exactly the reviewed migration observation. */
  async applySkillsMigration(
    intent: SkillsMigrationIntent,
  ): Promise<SkillsMigrationExecutionResult> {
    return await invoke("applySkillsMigration", { intent });
  },

  /** Reveal a path selected by the backend from a still-current migration plan. */
  async revealSkillsMigrationPlanItem(
    intent: SkillsMigrationRevealIntent,
  ): Promise<boolean> {
    return await invoke("revealSkillsMigrationPlanItem", { intent });
  },

  /** Continue backend-journaled work without rebuilding a plan in the UI. */
  async resumeSkillsMigration(): Promise<SkillsMigrationExecutionResult> {
    return await invoke("resumeSkillsMigration");
  },

  /** Restore by opaque backup identity; display paths never become intent. */
  async restoreSkillsMigrationBackup(
    backupId: string,
  ): Promise<SkillsMigrationExecutionResult> {
    return await invoke("restoreSkillsMigrationBackup", { backupId });
  },

  /** Apply ordered deployment intents without selecting an app implicitly. */
  async applyDeployments(
    batch: DeploymentBatch,
  ): Promise<DeploymentBatchResult> {
    return await invoke("applySkillDeployments", { batch });
  },

  /** Acquire a Git or marketplace Skill without enabling any consumer. */
  async acquireLibrary(
    skill: DiscoverableSkill,
    sourceKind: RemoteLibrarySourceKind,
    directoryName?: string,
  ): Promise<LibrarySkill> {
    return await invoke("acquireLibrarySkill", {
      skill,
      sourceKind,
      directoryName,
    });
  },

  /** Acquire all valid snapshots in a local ZIP into the private Library. */
  async acquireLibraryFromZip(
    filePath: string,
    directoryNames: Record<string, string> = {},
  ): Promise<LibrarySkill[]> {
    return await invoke("acquireLibrarySkillsFromZip", {
      filePath,
      directoryNames,
    });
  },

  /** Update display metadata; directory identity is deliberately absent. */
  async updateLibraryMetadata(
    id: string,
    displayName: string,
    description?: string,
  ): Promise<LibrarySkill> {
    return await invoke("updateLibrarySkillMetadata", {
      id,
      displayName,
      description,
    });
  },

  /** Check and stage an upstream Library snapshot without changing live content. */
  async checkLibrarySkillUpdate(
    librarySkillId: string,
  ): Promise<LibrarySkillUpdateCheckResult> {
    return await invoke("checkLibrarySkillUpdate", { librarySkillId });
  },

  /** Apply a previously staged snapshot after fresh-token and confirmation checks. */
  async applyLibrarySkillUpdate(
    intent: LibrarySkillUpdateIntent,
  ): Promise<LibrarySkillUpdateResult> {
    return await invoke("applyLibrarySkillUpdate", { intent });
  },

  /** Inspect all deployment records before attempting Library deletion. */
  async inspectLibrarySkillDeletion(
    librarySkillId: string,
  ): Promise<LibrarySkillDeletionInspection> {
    return await invoke("inspectLibrarySkillDeletion", { librarySkillId });
  },

  /** Delete a Library snapshot after explicit deployment reconciliation. */
  async deleteLibrarySkill(
    intent: LibrarySkillDeletionIntent,
  ): Promise<LibrarySkillDeletionResult> {
    return await invoke("deleteLibrarySkill", { intent });
  },

  /** 发现可安装的 Skills（从仓库获取） */
  async discoverAvailable(): Promise<DiscoverableSkill[]> {
    return await invoke("discover_available_skills");
  },

  /** 搜索 skills.sh 公共目录 */
  async searchSkillsSh(
    query: string,
    limit: number,
    offset: number,
  ): Promise<SkillsShSearchResult> {
    return await invoke("search_skills_sh", { query, limit, offset });
  },

  // ========== 仓库管理 ==========

  /** 获取仓库列表 */
  async getRepos(): Promise<SkillRepo[]> {
    return await invoke("get_skill_repos");
  },

  /** 添加仓库 */
  async addRepo(repo: SkillRepo): Promise<boolean> {
    return await invoke("add_skill_repo", { repo });
  },

  /** 删除仓库 */
  async removeRepo(owner: string, name: string): Promise<boolean> {
    return await invoke("remove_skill_repo", { owner, name });
  },

  // ========== ZIP 安装 ==========

  /** 打开 ZIP 文件选择对话框 */
  async openZipFileDialog(): Promise<string | null> {
    return await invoke("open_zip_file_dialog");
  },
};
