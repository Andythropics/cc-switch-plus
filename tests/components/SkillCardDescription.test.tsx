import { act, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SkillCardDescription } from "@/components/skills/SkillCardDescription";

it("adjusts the truncation to the available height and keeps the full description accessible", () => {
  let resize = () => {};
  let height = 84;
  const disconnect = vi.fn();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect = disconnect;
    },
  );
  const heightSpy = vi
    .spyOn(HTMLElement.prototype, "clientHeight", "get")
    .mockImplementation(() => height);
  const styleSpy = vi
    .spyOn(window, "getComputedStyle")
    .mockReturnValue({ lineHeight: "21px" } as CSSStyleDeclaration);
  try {
    const { unmount } = render(
      <SkillCardDescription text="A long description" />,
    );
    const description = screen.getByText("A long description");
    expect(description.style.webkitLineClamp).toBe("4");
    height = 42;
    act(() => resize());
    expect(description.style.webkitLineClamp).toBe("2");
    expect(description).toHaveAttribute("title", "A long description");
    unmount();
    expect(disconnect).toHaveBeenCalledOnce();
  } finally {
    heightSpy.mockRestore();
    styleSpy.mockRestore();
    vi.unstubAllGlobals();
  }
});
