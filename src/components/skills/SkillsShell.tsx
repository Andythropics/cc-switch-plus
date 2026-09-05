import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import {
  FolderOpen,
  Globe2,
  History,
  Library,
  Search,
  Wrench,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { SkillsDialogContent } from "@/components/skills/SkillsDialogContent";
import {
  isActionableDeploymentRecoveryFinding,
  DeploymentRecoveryPanel,
} from "@/components/skills/DeploymentRecoveryPanel";
import { useDeploymentRecovery } from "@/hooks/useSkills";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

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
  onInteractionBlockedChange?: (blocked: boolean) => void;
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
  onInteractionBlockedChange,
}: SkillsShellProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const globalRecoveryQuery = useDeploymentRecovery({ workspace: "global" });
  const [globalRecoveryOpen, setGlobalRecoveryOpen] = useState(false);
  const [globalRecoveryBusy, setGlobalRecoveryBusy] = useState(false);
  const hasActionableGlobalRecovery = (
    globalRecoveryQuery.data?.findings ?? []
  ).some(
    (finding) =>
      finding.target.workspace === "global" &&
      isActionableDeploymentRecoveryFinding(finding),
  );

  useEffect(() => {
    onInteractionBlockedChange?.(globalRecoveryBusy);
    return () => {
      onInteractionBlockedChange?.(false);
    };
  }, [globalRecoveryBusy, onInteractionBlockedChange]);

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
    <>
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
          {hasActionableGlobalRecovery && (
            <div className="ml-auto">
              <TooltipProvider delayDuration={250}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      data-testid="global-recovery-trigger"
                      aria-label={t("skills.recovery.open")}
                      disabled={
                        readOnly || navigationDisabled || globalRecoveryBusy
                      }
                      className="text-destructive hover:text-destructive"
                      onClick={() => setGlobalRecoveryOpen(true)}
                    >
                      <Wrench className="h-4 w-4 text-destructive" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    {t("skills.recovery.navTooltip")}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </div>
          )}
        </nav>
        <div
          ref={scrollRef}
          className="min-h-0 flex-1 overflow-y-auto"
          data-skills-scroll-root="true"
        >
          {children}
        </div>
      </div>
      <Dialog
        open={globalRecoveryOpen}
        onOpenChange={(open) => {
          if (!open && globalRecoveryBusy) return;
          setGlobalRecoveryOpen(open);
        }}
      >
        <SkillsDialogContent
          closeBlocked={globalRecoveryBusy}
          className="max-h-[90vh] max-w-2xl overflow-y-auto p-0"
        >
          <DialogTitle className="sr-only">
            {t("skills.recovery.title")}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("skills.recovery.description")}
          </DialogDescription>
          <DeploymentRecoveryPanel
            query={{ workspace: "global" }}
            onBusyChange={setGlobalRecoveryBusy}
          />
        </SkillsDialogContent>
      </Dialog>
    </>
  );
}
