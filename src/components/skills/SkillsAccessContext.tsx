import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  type PropsWithChildren,
  type ReactNode,
} from "react";

/**
 * The small capability surface shared by the Skills shell and its pages.
 *
 * Migration owns the decision about whether the page is writable. Keeping
 * that decision in a context lets pages opt into the same rule without
 * passing a read-only prop through every panel in the Skills tree.
 */
export interface SkillsAccessValue {
  readOnly: boolean;
  navigationDisabled: boolean;
  capabilities: {
    canMutate: boolean;
    canNavigate: boolean;
  };
}

const DEFAULT_ACCESS: SkillsAccessValue = {
  readOnly: false,
  navigationDisabled: false,
  capabilities: {
    canMutate: true,
    canNavigate: true,
  },
};

const SkillsAccessContext = createContext<SkillsAccessValue>(DEFAULT_ACCESS);

export function SkillsAccessProvider({
  children,
  readOnly = false,
  navigationDisabled = false,
}: PropsWithChildren<{
  readOnly?: boolean;
  navigationDisabled?: boolean;
}>) {
  const value = useMemo<SkillsAccessValue>(
    () => ({
      readOnly,
      navigationDisabled,
      capabilities: {
        canMutate: !readOnly && !navigationDisabled,
        canNavigate: !navigationDisabled,
      },
    }),
    [navigationDisabled, readOnly],
  );

  return (
    <SkillsAccessContext.Provider value={value}>
      {children}
    </SkillsAccessContext.Provider>
  );
}

export function useSkillsAccess() {
  return useContext(SkillsAccessContext);
}

/**
 * Keeps migration controls interactive while making a rendered Skills page
 * inert when migration has put the page in read-only mode. The attribute is
 * applied imperatively because React 18's DOM types do not include `inert`.
 */
export function SkillsAccessBoundary({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  const { readOnly } = useSkillsAccess();
  const nodeRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const node = nodeRef.current;
    if (!node) return;

    if (readOnly) {
      node.setAttribute("inert", "");
    } else {
      node.removeAttribute("inert");
    }
  }, [readOnly]);

  return (
    <div
      ref={nodeRef}
      className={className}
      aria-readonly={readOnly || undefined}
      data-skills-access-boundary="true"
    >
      {children}
    </div>
  );
}
