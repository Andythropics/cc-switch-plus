import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

const repo: SkillRepo = {
  owner: "acme",
  name: "skill-pack",
  branch: "stable",
  enabled: true,
};

const skills = Array.from(
  { length: 3 },
  (_, index) =>
    ({
      name: `Skill ${index + 1}`,
      description: "",
      repoOwner: repo.owner,
      repoName: repo.name,
      repoBranch: repo.branch,
    }) as DiscoverableSkill,
);

describe("RepoManagerPanel", () => {
  it("requires confirmation before removing a repository", async () => {
    let resolveRemoval: (() => void) | undefined;
    const onRemove = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveRemoval = resolve;
        }),
    );
    const user = userEvent.setup();

    render(
      <RepoManagerPanel
        repos={[repo]}
        skills={skills}
        onAdd={vi.fn()}
        onRemove={onRemove}
        onClose={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.repo.removeAction" }),
    );

    expect(onRemove).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toHaveTextContent("acme/skill-pack");
    expect(screen.getByRole("dialog")).toHaveTextContent("stable");
    expect(screen.getByRole("dialog")).toHaveTextContent("3");

    await user.click(
      screen.getByRole("button", { name: "skills.repo.removeCancel" }),
    );
    expect(onRemove).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.repo.removeAction" }),
    );
    const confirm = screen.getByRole("button", {
      name: "skills.repo.removeConfirm",
    });
    await user.click(confirm);

    expect(onRemove).toHaveBeenCalledTimes(1);
    expect(onRemove).toHaveBeenCalledWith("acme", "skill-pack");
    expect(confirm).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.repo.removeCancel" }),
    ).toBeDisabled();

    resolveRemoval?.();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });
});
