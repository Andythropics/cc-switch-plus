import { useLayoutEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Globe2, History, Library, Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { SkillsAccessProvider } from "@/components/skills/SkillsAccessContext";

export type SkillsView =
  | "skills"
  | "skillsGlobal"
  | "skillsProjects"
  | "skillsDiscovery"
  | "skillsActivity";

interface SkillsShellProps {
  view: SkillsView;
  onViewChange: (view: SkillsView) => void;
  children: ReactNode;
  readOnly?: boolean;
  navigationDisabled?: boolean;
}

const NAV_ITEMS = [
  { view: "skills" as const, labelKey: "skills.library.title", Icon: Library },
  {
    view: "skillsGlobal" as const,
    labelKey: "skills.global.title",
    Icon: Globe2,
  },
  {
    view: "skillsProjects" as const,
    labelKey: "skills.projects.title",
    Icon: FolderOpen,
  },
  {
    view: "skillsDiscovery" as const,
    labelKey: "skills.discover",
    Icon: Search,
  },
  {
    view: "skillsActivity" as const,
    labelKey: "skills.activity.title",
    Icon: History,
  },
] as const;

export function isSkillsView(view: string): view is SkillsView {
  return NAV_ITEMS.some((item) => item.view === view);
}

/**
 * Shared frame for every Skills surface. It owns the navigation seam and the
 * scroll viewport, while the individual panels remain responsible for their
 * domain-specific content and mutations.
 */
export function SkillsShell({
  view,
  onViewChange,
  children,
  readOnly = false,
  navigationDisabled = false,
}: SkillsShellProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const scrollNode = scrollRef.current;
    if (!scrollNode) return;

    if (typeof scrollNode.scrollTo === "function") {
      scrollNode.scrollTo({ top: 0, left: 0, behavior: "auto" });
    } else {
      scrollNode.scrollTop = 0;
      scrollNode.scrollLeft = 0;
    }
  }, [view]);

  return (
    <SkillsAccessProvider
      readOnly={readOnly}
      navigationDisabled={navigationDisabled}
    >
      <div className="flex h-full min-h-0 flex-col">
        <nav
          aria-label={t("skills.manage")}
          className="flex shrink-0 flex-wrap items-center gap-1 border-b px-5 py-2"
          data-skills-navigation="true"
        >
          {NAV_ITEMS.map(({ view: itemView, labelKey, Icon }) => {
            const active = itemView === view;
            return (
              <Button
                key={itemView}
                type="button"
                size="sm"
                variant={active ? "secondary" : "ghost"}
                disabled={navigationDisabled || active}
                aria-current={active ? "page" : undefined}
                aria-label={t(labelKey)}
                onClick={() => onViewChange(itemView)}
                className={cn(
                  "gap-2",
                  active && "bg-primary/10 text-primary hover:bg-primary/15",
                )}
              >
                <Icon className="h-4 w-4" />
                <span>{t(labelKey)}</span>
              </Button>
            );
          })}
        </nav>
        <div
          ref={scrollRef}
          className="min-h-0 flex-1 overflow-y-auto"
          data-skills-scroll-root="true"
        >
          {children}
        </div>
      </div>
    </SkillsAccessProvider>
  );
}
