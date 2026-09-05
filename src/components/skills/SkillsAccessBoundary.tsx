import type { ReactNode } from "react";

/**
 * Keeps migration controls interactive while making a rendered Skills page
 * inert when migration has put the page in read-only mode. The attribute is
 * applied imperatively because React 18's DOM types do not include `inert`.
 */
export function SkillsAccessBoundary({
  children,
  className,
  readOnly,
}: {
  children: ReactNode;
  className?: string;
  readOnly: boolean;
}) {
  return (
    <div
      ref={(node) => {
        if (node) node.inert = readOnly;
      }}
      className={className}
      aria-readonly={readOnly || undefined}
    >
      {children}
    </div>
  );
}
