import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";

describe("Dialog", () => {
  it.each([
    ["base", "z-[60]"],
    ["nested", "z-[70]"],
    ["alert", "z-[80]"],
    ["top", "z-[110]"],
  ] as const)("renders the %s layer above app chrome", (zIndex, className) => {
    const { unmount } = render(
      <Dialog open>
        <DialogContent
          zIndex={zIndex}
          data-testid="dialog-content"
          overlayClassName="dialog-overlay"
        >
          <DialogTitle>Dialog</DialogTitle>
          <DialogDescription>Description</DialogDescription>
        </DialogContent>
      </Dialog>,
    );

    expect(document.querySelector(".dialog-overlay")).toHaveClass(className);
    expect(
      document.querySelector('[data-testid="dialog-content"]'),
    ).toHaveClass(className);
    unmount();
  });
});
