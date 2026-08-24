import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SkillTechnicalDetails } from "@/components/skills/SkillTechnicalDetails";

describe("SkillTechnicalDetails", () => {
  it("keeps backend diagnostics collapsed until explicitly requested", () => {
    render(
      <SkillTechnicalDetails details="internal backend path /tmp/private" />,
    );

    const disclosure = screen
      .getByText("skills.error.technicalDetails")
      .closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(disclosure).toHaveTextContent("internal backend path /tmp/private");
  });
});
