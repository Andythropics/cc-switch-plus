import { describe, expect, it, vi } from "vitest";

import { formatSkillError } from "@/lib/errors/skillErrorParser";

describe("formatSkillError", () => {
  const t = vi.fn((key: string) => key);

  it("keeps an unstructured backend error out of the user-facing summary", () => {
    const raw = "sqlite failure at /Users/example/private/library.db";

    expect(formatSkillError(raw, t as never, "skills.repo.addFailed")).toEqual({
      title: "skills.repo.addFailed",
      description: "skills.error.unknownError",
      technicalDetails: raw,
    });
  });

  it("localizes a structured backend error and preserves the payload as diagnostics", () => {
    const raw = JSON.stringify({
      code: "SKILL_NOT_FOUND",
      context: { directory: "missing-skill" },
    });

    expect(
      formatSkillError(raw, t as never, "skills.library.acquireFailed"),
    ).toEqual({
      title: "skills.library.acquireFailed",
      description: "skills.error.skillNotFound",
      technicalDetails: raw,
    });
  });
});
