import { invoke } from "@tauri-apps/api/core";
import type {
  LibrarySkill,
  LibrarySkillSource,
  LibrarySkillUpdateResult,
} from "./skills";

export interface ExternalSkillCandidate {
  targetObservationTokens?: Record<string, string>;
  id: string;
  directory: string;
  source: LibrarySkillSource;
  contentHash: string;
  librarySkillId: string | null;
  suggestedLibrarySkillIds: string[];
  changed: boolean;
  localModified: boolean;
  deploymentReplaced?: boolean;
  fullySynced?: boolean;
}

export interface ExternalSkillInspection {
  observationToken: string;
  candidates: ExternalSkillCandidate[];
  warnings: string[];
}

export interface ExternalSkillIntent {
  candidateId: string;
  librarySkillId: string;
  observationToken: string;
  confirmLocalModifications: boolean;
  restoreDeployment?: boolean;
}

export const externalSkillUpdatesApi = {
  inspect: () => invoke<ExternalSkillInspection>("inspectExternalSkillUpdates"),
  link: (intent: ExternalSkillIntent) =>
    invoke<LibrarySkill>("linkExternalSkillSource", { intent }),
  apply: (intent: ExternalSkillIntent) =>
    invoke<LibrarySkillUpdateResult>("applyExternalSkillUpdate", { intent }),
  linkSource: (intent: {
    librarySkillId: string;
    source: LibrarySkillSource;
    expectedContentHash: string;
  }) => invoke<LibrarySkill>("linkLibrarySkillSource", { intent }),
};
