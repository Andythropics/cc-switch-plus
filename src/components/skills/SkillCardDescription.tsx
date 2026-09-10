import { useLayoutEffect, useRef, useState } from "react";

/** Clamp to the space left by the card header, while retaining breathing room. */
export function SkillCardDescription({ text }: { text: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const paragraphRef = useRef<HTMLParagraphElement>(null);
  const [lines, setLines] = useState(1);

  useLayoutEffect(() => {
    const container = containerRef.current;
    const paragraph = paragraphRef.current;
    if (!container || !paragraph) return;
    const measure = () => {
      const lineHeight = parseFloat(getComputedStyle(paragraph).lineHeight);
      if (lineHeight > 0) {
        setLines(Math.max(1, Math.floor(container.clientHeight / lineHeight)));
      }
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    return () => observer.disconnect();
  }, [text]);

  return (
    <div className="min-h-0 flex-1 px-1 py-2">
      <div
        ref={containerRef}
        className="flex h-full min-h-0 items-center overflow-hidden"
      >
        <p
          ref={paragraphRef}
          className="w-full overflow-hidden break-words text-sm leading-relaxed text-muted-foreground/90"
          style={{
            display: "-webkit-box",
            WebkitBoxOrient: "vertical",
            WebkitLineClamp: lines,
          }}
          title={text}
        >
          {text}
        </p>
      </div>
    </div>
  );
}
