import { invoke } from "@tauri-apps/api/core";

import type { DeploymentConsumer, LibrarySkillCompatibility } from "./skills";
import type {
  ProjectSkillImportDirectoryCollision,
  ProjectSkillImportLibraryMatch,
  ProjectSkillImportMode,
  ProjectSkillImportOutcome,
  ProjectSkillImportReplaceEligibility,
  ProjectSkillImportResolution,
  ProjectSkillImportValidation,
} from "./projectWorkspaces";

export interface GlobalSkillImportFinding {
  id: string;
  consumer: DeploymentConsumer;
  /** Display-only; apply accepts identity + observation token, never a path. */
  sourcePath: string;
  directory: string;
  validation: ProjectSkillImportValidation;
  compatibility: LibrarySkillCompatibility;
  libraryMatch: ProjectSkillImportLibraryMatch;
  directoryCollision: ProjectSkillImportDirectoryCollision;
  replaceEligibility: ProjectSkillImportReplaceEligibility;
}

export interface GlobalSkillImportInspection {
  observationToken: string;
  findings: GlobalSkillImportFinding[];
}

export type GlobalSkillImportMode = ProjectSkillImportMode;
export type GlobalSkillImportResolution = ProjectSkillImportResolution;
export type GlobalSkillImportOutcome = ProjectSkillImportOutcome;

export interface GlobalSkillImportIntent {
  findingId: string;
  observationToken: string;
  mode: GlobalSkillImportMode;
  resolution: GlobalSkillImportResolution;
}

export interface GlobalSkillImportResult {
  findingId: string;
  outcome: GlobalSkillImportOutcome;
  librarySkillId?: string;
  directory?: string;
  reason?: GlobalSkillImportFinding["replaceEligibility"]["reason"];
  message?: string;
  backupPath?: string;
}

export const globalSkillImportsApi = {
  async inspect(): Promise<GlobalSkillImportInspection> {
    return await invoke("inspectGlobalSkillImports");
  },

  async apply(
    intent: GlobalSkillImportIntent,
  ): Promise<GlobalSkillImportResult> {
    return await invoke("applyGlobalSkillImport", { intent });
  },
};
