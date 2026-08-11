import { createRef } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  LibrarySkillsPanel,
  type LibrarySkillsPanelHandle,
} from "@/components/skills/LibrarySkillsPanel";
import type { LibrarySkill } from "@/lib/api/skills";

const { updateMetadataMock, acquireZipMock, openZipMock } = vi.hoisted(() => ({
  updateMetadataMock: vi.fn(),
  acquireZipMock: vi.fn(),
  openZipMock: vi.fn(),
}));

const librarySkill: LibrarySkill = {
  id: "library-1",
  directory: "review-skill",
  displayName: "Careful review",
  description: "Review changes carefully",
  source: {
    kind: "marketplace",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    skillPath: "skills/review",
    marketplace: "skills.sh",
  },
  compatibility: {
    claude: { compatible: true, issues: [] },
    codex: { compatible: true, issues: [] },
  },
  contentHash: "abc",
  acquiredAt: 1,
  updatedAt: 1,
};

vi.mock("@/hooks/useSkills", () => ({
  useLibrarySkills: () => ({ data: [librarySkill], isLoading: false }),
  useUpdateLibrarySkillMetadata: () => ({
    mutateAsync: updateMetadataMock,
    isPending: false,
  }),
  useAcquireLibrarySkillsFromZip: () => ({
    mutateAsync: acquireZipMock,
    isPending: false,
  }),
}));

vi.mock("@/lib/api", () => ({
  skillsApi: { openZipFileDialog: openZipMock },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

describe("LibrarySkillsPanel", () => {
  beforeEach(() => {
    updateMetadataMock.mockReset().mockResolvedValue(librarySkill);
    acquireZipMock.mockReset().mockResolvedValue([librarySkill]);
    openZipMock.mockReset().mockResolvedValue("/tmp/skill.zip");
  });

  it("shows immutable identity, source, compatibility, and edits display metadata", async () => {
    render(<LibrarySkillsPanel onOpenDiscovery={vi.fn()} />);

    expect(screen.getByText("Careful review")).toBeInTheDocument();
    expect(screen.getByText("review-skill")).toBeInTheDocument();
    expect(screen.getByText(/owner\/repo/)).toBeInTheDocument();
    expect(screen.getByText(/skills\/review/)).toBeInTheDocument();
    expect(
      screen.getByText("skills.library.consumerClaude"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("skills.library.consumerCodex"),
    ).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "skills.library.edit" }),
    );
    const name = screen.getByLabelText("skills.library.displayName");
    await user.clear(name);
    await user.type(name, "Review teammate");
    await user.click(
      screen.getByRole("button", { name: "skills.library.save" }),
    );

    await waitFor(() =>
      expect(updateMetadataMock).toHaveBeenCalledWith({
        id: "library-1",
        displayName: "Review teammate",
        description: "Review changes carefully",
      }),
    );
    expect(updateMetadataMock.mock.calls[0][0]).not.toHaveProperty("directory");
  });

  it("acquires a ZIP through the Library-only imperative action", async () => {
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} onOpenDiscovery={vi.fn()} />);

    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });

    expect(acquireZipMock).toHaveBeenCalledWith({
      filePath: "/tmp/skill.zip",
      directoryNames: {},
    });
  });

  it("requires an explicit unique directory when ZIP acquisition collides", async () => {
    acquireZipMock
      .mockRejectedValueOnce(
        new Error(
          "LIBRARY_DIRECTORY_CONFLICT: 'review-skill' is already in use",
        ),
      )
      .mockResolvedValueOnce([librarySkill]);
    const ref = createRef<LibrarySkillsPanelHandle>();
    render(<LibrarySkillsPanel ref={ref} onOpenDiscovery={vi.fn()} />);

    await act(async () => {
      await ref.current?.openAcquireFromZip();
    });

    const user = userEvent.setup();
    const directory = await screen.findByLabelText("skills.library.directory");
    await user.clear(directory);
    await user.type(directory, "review-skill-copy");
    await user.click(
      screen.getByRole("button", { name: "skills.library.acquire" }),
    );

    await waitFor(() =>
      expect(acquireZipMock).toHaveBeenLastCalledWith({
        filePath: "/tmp/skill.zip",
        directoryNames: { "review-skill": "review-skill-copy" },
      }),
    );
  });
});
