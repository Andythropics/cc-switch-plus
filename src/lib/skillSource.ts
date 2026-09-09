import type { LibrarySkillSource } from "./api/skills";

/** Repository paths are explicit: a marketplace slug is not a repository path. */
export function sourceFromInput(
  raw: string,
  branch: string,
  path: string,
): LibrarySkillSource {
  const url = new URL(
    raw.includes("://") ? raw.trim() : `https://github.com/${raw.trim()}`,
  );
  const parts = url.pathname.split("/").filter(Boolean);
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    url.port ||
    url.search ||
    url.hash ||
    !["github.com", "skills.sh"].includes(url.hostname) ||
    (url.hostname === "github.com" ? parts.length !== 2 : parts.length !== 3)
  ) {
    throw new Error("invalid_source");
  }
  if (!path.trim()) throw new Error("missing_path");
  const repoName = parts[1].replace(/\.git$/, "");
  return {
    kind: url.hostname === "skills.sh" ? "marketplace" : "git",
    url: `https://github.com/${parts[0]}/${repoName}`,
    repoOwner: parts[0],
    repoName,
    repoBranch: branch.trim() || undefined,
    skillPath:
      path.trim() === "SKILL.md"
        ? "."
        : path.trim().replace(/\/SKILL\.md$/, ""),
    marketplace: url.hostname === "skills.sh" ? "skills.sh" : undefined,
  };
}
