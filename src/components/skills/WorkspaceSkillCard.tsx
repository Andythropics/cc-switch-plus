import type { ReactNode } from "react";
import type { LibrarySkill } from "@/lib/api/skills";
import { Badge } from "@/components/ui/badge";
import { SkillCardDescription } from "@/components/skills/SkillCardDescription";
export const workspaceSkillGridClassName =
  "grid grid-cols-[repeat(auto-fit,minmax(min(100%,20rem),1fr))] items-start gap-3";
export const sourceSummary = (skill: LibrarySkill) => {
  if (skill.source.repoOwner && skill.source.repoName)
    return `${skill.source.repoOwner}/${skill.source.repoName}`;
  return skill.source.url;
};
/** Shared presentation for Global and Project Workspace deployments. */
export function WorkspaceSkillCard({
  skill,
  children,
}: {
  skill: LibrarySkill;
  children: ReactNode;
}) {
  return (
    <article className="glass-card skill-surface-card flex h-48 min-w-0 flex-col overflow-hidden rounded-xl border p-4">
      <div className="flex max-h-[50%] shrink-0 flex-wrap items-start gap-3 overflow-y-auto">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="min-w-0 break-words text-sm font-semibold">
              {skill.displayName}
            </h3>
            <Badge
              variant="outline"
              className="min-h-5 max-w-full whitespace-normal break-all border-border-default px-2 py-0 text-left font-mono text-[11px]"
            >
              {skill.directory}
            </Badge>
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            {sourceSummary(skill) && (
              <span className="min-w-0 break-all">{sourceSummary(skill)}</span>
            )}
          </div>
        </div>
        <div className="flex max-w-full shrink-0 flex-wrap items-center gap-2">
          {children}
        </div>
      </div>
      {skill.description && <SkillCardDescription text={skill.description} />}
    </article>
  );
}
