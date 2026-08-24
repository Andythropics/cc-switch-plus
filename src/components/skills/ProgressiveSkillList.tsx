import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

export const SKILL_LIST_PAGE_SIZE = 40;

export function useProgressiveSkillList<T>(items: T[], resetKey: string) {
  const [limit, setLimit] = useState(SKILL_LIST_PAGE_SIZE);

  useEffect(() => {
    setLimit(SKILL_LIST_PAGE_SIZE);
  }, [resetKey]);

  const visibleCount = Math.min(limit, items.length);
  const visibleItems = useMemo(
    () => items.slice(0, visibleCount),
    [items, visibleCount],
  );

  return {
    visibleItems,
    visibleCount,
    totalCount: items.length,
    hasMore: visibleCount < items.length,
    showMore: () => setLimit((current) => current + SKILL_LIST_PAGE_SIZE),
  };
}

export function ProgressiveSkillListFooter({
  visibleCount,
  totalCount,
  hasMore,
  onShowMore,
}: {
  visibleCount: number;
  totalCount: number;
  hasMore: boolean;
  onShowMore: () => void;
}) {
  const { t } = useTranslation();
  if (totalCount <= SKILL_LIST_PAGE_SIZE) return null;

  return (
    <div
      className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-dashed p-2 text-xs text-muted-foreground"
      data-testid="skills-list-progress"
      data-visible-count={visibleCount}
      data-total-count={totalCount}
    >
      <span>
        {t("skills.list.showing", {
          visible: visibleCount,
          total: totalCount,
        })}
      </span>
      {hasMore && (
        <Button type="button" size="sm" variant="outline" onClick={onShowMore}>
          {t("skills.list.showMore")}
        </Button>
      )}
    </div>
  );
}
