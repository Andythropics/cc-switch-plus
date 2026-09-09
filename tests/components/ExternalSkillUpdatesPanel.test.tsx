import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ExternalSkillUpdatesPanel } from "@/components/skills/ExternalSkillUpdatesPanel";
import { sourceFromInput } from "@/lib/skillSource";
import { LinkSkillSourceDialog } from "@/components/skills/LinkSkillSourceDialog";
import type { ExternalSkillCandidate } from "@/lib/api/externalSkillUpdates";
import type { LibrarySkill } from "@/lib/api/skills";

const state = vi.hoisted(() => ({
  candidates: [] as ExternalSkillCandidate[],
  warnings: [] as string[],
  link: vi.fn(),
  apply: vi.fn(),
  linkSource: vi.fn(),
  refetch: vi.fn(),
}));
vi.mock("@/hooks/useExternalSkillUpdates", () => ({
  useLinkLibrarySkillSource: () => ({ mutateAsync: state.linkSource }),
  useExternalSkillUpdates: () => ({
    inspection: {
      data: {
        observationToken: "observed",
        candidates: state.candidates,
        warnings: state.warnings,
      },
      refetch: state.refetch,
    },
    link: { mutateAsync: state.link },
    apply: { mutateAsync: state.apply },
    linkSource: { mutateAsync: state.linkSource },
  }),
}));
const skill: LibrarySkill = {
  id: "implementation-library",
  directory: "implementation",
  displayName: "Implementation",
  source: { kind: "local_import" },
  contentHash: "old",
  acquiredAt: 1,
  updatedAt: 1,
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
};
const candidate: ExternalSkillCandidate = {
  id: "implementation",
  directory: "implementation",
  source: {
    kind: "git",
    repoOwner: "owner",
    repoName: "repo",
    skillPath: "skills/implementation",
  },
  contentHash: "new",
  librarySkillId: null,
  suggestedLibrarySkillIds: [skill.id],
  changed: true,
  localModified: false,
};
async function openCandidate() {
  fireEvent.click(
    screen.getByRole("button", { name: "skills.external.syncCli" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "skills.external.select" }),
  );
}
beforeEach(() => {
  state.candidates = [{ ...candidate }];
  state.warnings = [];
  state.link.mockResolvedValue(skill);
  state.apply.mockResolvedValue({ outcome: "updated" });
  state.linkSource.mockResolvedValue(skill);
});

describe("external Skill identity confirmation", () => {
  it("keeps fully synced installations hidden after a rescan", async () => {
    state.candidates = [{ ...candidate, fullySynced: true }];
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.syncCli" }),
    );
    expect(screen.getByText("skills.external.allSynced")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.external.select" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.rescan" }),
    );
    expect(state.refetch).toHaveBeenCalledTimes(2);
    expect(screen.getByText("skills.external.allSynced")).toBeInTheDocument();
  });
  it("keeps repair warnings visible instead of reporting everything synced", () => {
    state.candidates = [{ ...candidate, fullySynced: true }];
    state.warnings = ["broken CLI link"];
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.syncCli" }),
    );
    expect(screen.getByText("broken CLI link")).toBeInTheDocument();
    expect(screen.getByText("skills.external.empty")).toBeInTheDocument();
    expect(
      screen.queryByText("skills.external.allSynced"),
    ).not.toBeInTheDocument();
  });
  it("retains matching copies and deployments that still need attention", () => {
    state.candidates = [
      { ...candidate, id: "complete", fullySynced: true },
      {
        ...candidate,
        librarySkillId: skill.id,
        changed: false,
        deploymentReplaced: false,
        fullySynced: false,
      },
    ];
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.syncCli" }),
    );
    expect(
      screen.getAllByRole("button", { name: "skills.external.select" }),
    ).toHaveLength(1);
    expect(
      screen.queryByText("skills.external.allSynced"),
    ).not.toBeInTheDocument();
  });
  it("prioritizes the directory match and allows changing the recommendation", async () => {
    const other = { ...skill, id: "other", directory: "other-directory" };
    state.candidates = [
      { ...candidate, suggestedLibrarySkillIds: [other.id, skill.id] },
    ];
    render(<ExternalSkillUpdatesPanel skills={[other, skill]} />);
    await openCandidate();
    const select = screen.getByLabelText("skills.external.target");
    expect(select).toHaveValue(skill.id);
    expect(
      screen.getByText("skills.external.recommendation.directory"),
    ).toBeInTheDocument();
    fireEvent.change(select, { target: { value: other.id } });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkOnly" }),
    );
    await waitFor(() =>
      expect(state.link).toHaveBeenCalledWith(
        expect.objectContaining({ librarySkillId: other.id }),
      ),
    );
  });
  it("recommends the sole name match when its directory differs", async () => {
    render(
      <ExternalSkillUpdatesPanel
        skills={[{ ...skill, directory: "renamed-directory" }]}
      />,
    );
    await openCandidate();
    expect(screen.getByLabelText("skills.external.target")).toHaveValue(
      skill.id,
    );
    expect(
      screen.getByText("skills.external.recommendation.name"),
    ).toBeInTheDocument();
    expect(state.link).not.toHaveBeenCalled();
  });
  it("does not pick arbitrarily between equally ranked name matches", async () => {
    const a = { ...skill, directory: "first" };
    const b = { ...skill, id: "second", directory: "second" };
    state.candidates = [
      { ...candidate, suggestedLibrarySkillIds: [a.id, b.id] },
    ];
    render(<ExternalSkillUpdatesPanel skills={[a, b]} />);
    await openCandidate();
    expect(screen.getByLabelText("skills.external.target")).toHaveValue("");
    expect(
      screen.getByText("skills.external.recommendation.ambiguous"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skills.external.linkAndUpdate" }),
    ).toBeDisabled();
  });
  it("leaves unmatched or missing suggestions unselected", async () => {
    state.candidates = [
      { ...candidate, suggestedLibrarySkillIds: ["missing"] },
    ];
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    await openCandidate();
    expect(screen.getByLabelText("skills.external.target")).toHaveValue("");
    expect(state.apply).not.toHaveBeenCalled();
  });
  it("offers only explicit CLI sync outside the cards and scans on click", () => {
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    expect(state.refetch).not.toHaveBeenCalled();
    expect(
      screen.queryByText("skills.external.manual"),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.syncCli" }),
    );
    expect(state.refetch).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "skills.external.manual" }),
    ).not.toBeInTheDocument();
  });
  it("prefills an existing card source and keeps marketplace provenance", async () => {
    render(
      <LinkSkillSourceDialog
        skill={{
          ...skill,
          source: {
            kind: "marketplace",
            repoOwner: "owner",
            repoName: "repo",
            repoBranch: "release",
            skillPath: "skills/implementation",
            marketplace: "skills.sh",
          },
        }}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("skills.external.sourceUrl")).toHaveValue(
      "https://github.com/owner/repo",
    );
    expect(screen.getByLabelText("skills.external.branch")).toHaveValue(
      "release",
    );
    expect(screen.getByLabelText("skills.external.path")).toHaveValue(
      "skills/implementation",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkOnly" }),
    );
    await waitFor(() =>
      expect(state.linkSource).toHaveBeenCalledWith(
        expect.objectContaining({
          librarySkillId: skill.id,
          source: expect.objectContaining({
            kind: "marketplace",
            marketplace: "skills.sh",
          }),
        }),
      ),
    );
    expect(state.refetch).not.toHaveBeenCalled();
  });
  it("preserves successful-update warnings and backup location", async () => {
    state.apply.mockResolvedValue({
      outcome: "updated",
      message: "Stage cleanup needs attention",
      backupPath: "/backup/implementation",
    });
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    await openCandidate();
    fireEvent.change(screen.getByLabelText("skills.external.target"), {
      target: { value: skill.id },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkAndUpdate" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Stage cleanup needs attention",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "skills.external.backup",
    );
  });
  it("preselects an editable recommendation without linking or overwriting", async () => {
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    await openCandidate();
    expect(screen.getByLabelText("skills.external.target")).toHaveValue(
      skill.id,
    );
    expect(screen.getByLabelText("skills.external.target")).toBeEnabled();
    expect(state.link).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "skills.external.linkAndUpdate" }),
    ).toBeEnabled();
    expect(state.apply).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("skills.external.target"), {
      target: { value: skill.id },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkOnly" }),
    );
    await waitFor(() =>
      expect(state.link).toHaveBeenCalledWith({
        candidateId: candidate.id,
        librarySkillId: skill.id,
        observationToken: "observed",
        confirmLocalModifications: false,
        restoreDeployment: false,
      }),
    );
    expect(state.apply).not.toHaveBeenCalled();
  });

  it("preselects confirmed identity and requires consent for local edits", async () => {
    state.candidates = [
      {
        ...candidate,
        librarySkillId: skill.id,
        localModified: true,
        deploymentReplaced: true,
      },
    ];
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    await openCandidate();
    expect(screen.getByLabelText("skills.external.target")).toHaveValue(
      skill.id,
    );
    expect(screen.getByLabelText("skills.external.target")).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.external.update" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.update" }),
    );
    await waitFor(() =>
      expect(state.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          librarySkillId: skill.id,
          confirmLocalModifications: true,
          restoreDeployment: true,
        }),
      ),
    );
  });

  it("keeps stale or failed updates visible without claiming success", async () => {
    state.apply.mockResolvedValue({
      outcome: "stale",
      message: "Snapshot changed",
    });
    render(<ExternalSkillUpdatesPanel skills={[skill]} />);
    await openCandidate();
    fireEvent.change(screen.getByLabelText("skills.external.target"), {
      target: { value: skill.id },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkAndUpdate" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Snapshot changed",
    );
    expect(
      screen.queryByText("skills.external.updated"),
    ).not.toBeInTheDocument();
  });

  it("manual association preserves content and passes an explicit repository path", async () => {
    render(<LinkSkillSourceDialog skill={skill} onClose={vi.fn()} />);
    expect(
      screen.queryByLabelText("skills.external.target"),
    ).not.toBeInTheDocument();
    expect(state.refetch).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("skills.external.sourceUrl"), {
      target: { value: "https://skills.sh/owner/repo/implementation" },
    });
    fireEvent.change(screen.getByLabelText("skills.external.path"), {
      target: { value: "skills/implementation" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "skills.external.linkOnly" }),
    );
    await waitFor(() =>
      expect(state.linkSource).toHaveBeenCalledWith({
        librarySkillId: skill.id,
        expectedContentHash: "old",
        source: expect.objectContaining({
          kind: "marketplace",
          repoOwner: "owner",
          repoName: "repo",
          skillPath: "skills/implementation",
        }),
      }),
    );
    expect(state.apply).not.toHaveBeenCalled();
  });
});

describe("manual upstream input", () => {
  it("normalizes a root SKILL.md to the repository root", () => {
    expect(sourceFromInput("owner/repo", "", "SKILL.md").skillPath).toBe(".");
  });
  it("keeps marketplace slug distinct from repository Skill path", () => {
    expect(
      sourceFromInput(
        "https://skills.sh/acme/tools/implementation",
        "release",
        "engineering/implement/SKILL.md",
      ),
    ).toMatchObject({
      repoOwner: "acme",
      repoName: "tools",
      repoBranch: "release",
      skillPath: "engineering/implement",
      marketplace: "skills.sh",
    });
  });
  it.each([
    "https://github.com.evil.test/owner/repo",
    "https://user:secret@github.com/owner/repo",
    "http://github.com/owner/repo",
    "https://github.com/owner/repo/tree/main/a",
  ])("rejects ambiguous or invalid source %s", (url) => {
    expect(() => sourceFromInput(url, "", ".")).toThrow();
  });
});
