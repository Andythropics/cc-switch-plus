import { describe, expect, it } from "vitest";

import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

type TranslationTree = Record<string, unknown>;

const locales = [
  ["en", en.skills.migration],
  ["ja", ja.skills.migration],
  ["zh", zh.skills.migration],
  ["zh-TW", zhTW.skills.migration],
] as const;

const requiredExecutionKeys = [
  "apply",
  "resume",
  "restore",
  "confirm.title",
  "confirm.description",
  "confirm.apply",
  "pending.applying",
  "pending.resuming",
  "pending.restoring",
  "execution.completed",
  "execution.stale_observation",
  "execution.resumable",
  "execution.blocked",
  "execution.recovery_required",
  "execution.restored",
  "execution.awaitingPreview",
  "execution.progress",
  "execution.commandError",
  "execution.blockedNoBackup",
  "itemOutcome.completed",
  "itemOutcome.already_completed",
  "itemOutcome.preserved",
  "itemOutcome.rolled_back",
  "itemOutcome.blocked",
  "itemOutcome.recovery_required",
  "action.finalize",
];

function leafPaths(tree: TranslationTree, prefix = ""): string[] {
  return Object.entries(tree).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value && typeof value === "object"
      ? leafPaths(value as TranslationTree, path)
      : [path];
  });
}

describe("Skills migration locale coverage", () => {
  it.each(locales)("exposes every execution key in %s", (_, tree) => {
    expect(leafPaths(tree)).toEqual(
      expect.arrayContaining(requiredExecutionKeys),
    );
  });

  it.each(locales)(
    "matches the complete migration key set in %s",
    (_, tree) => {
      expect(leafPaths(tree).sort()).toEqual(
        leafPaths(en.skills.migration).sort(),
      );
    },
  );

  it("describes backup readiness as prerequisites, not a completed backup", () => {
    expect(en.skills.migration.backup.ready).toBe(
      "Backup prerequisites are ready; no backup has been created yet.",
    );
    expect(en.skills.migration.backup.notReady).toBe(
      "Backup prerequisites are not ready; migration cannot be applied.",
    );
    expect(zh.skills.migration.backup.ready).toContain("备份前置条件");
    expect(zhTW.skills.migration.backup.ready).toContain("備份前置條件");
    expect(ja.skills.migration.backup.ready).toContain(
      "バックアップの前提条件",
    );
  });
});
