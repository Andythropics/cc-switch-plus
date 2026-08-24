import type { ComponentPropsWithoutRef } from "react";

import { DialogContent } from "@/components/ui/dialog";

type SkillsDialogContentProps = ComponentPropsWithoutRef<
  typeof DialogContent
> & {
  /** Blocks implicit dismissal while work is pending or editable state is dirty. */
  closeBlocked?: boolean;
};

/**
 * Skills dialogs share one dismissal contract:
 * - Escape and backdrop clicks close a safe, idle dialog.
 * - Pending work or unsaved input blocks those implicit dismissal paths.
 * - An explicit Cancel button remains responsible for intentional discard.
 */
export function SkillsDialogContent({
  closeBlocked = false,
  onEscapeKeyDown,
  onInteractOutside,
  ...props
}: SkillsDialogContentProps) {
  return (
    <DialogContent
      data-close-blocked={closeBlocked || undefined}
      onEscapeKeyDown={(event) => {
        onEscapeKeyDown?.(event);
        if (closeBlocked) event.preventDefault();
      }}
      onInteractOutside={(event) => {
        onInteractOutside?.(event);
        if (closeBlocked) event.preventDefault();
      }}
      {...props}
    />
  );
}
