import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import App from "@/App";

const { Empty } = vi.hoisted(() => ({ Empty: () => null }));

vi.mock("@/lib/platform", () => ({
  isMac: () => true,
  isLinux: () => false,
  isWindows: () => false,
  DRAG_REGION_ATTR: {},
  DRAG_REGION_STYLE: {},
}));

vi.mock("@/lib/query", () => ({
  proxyKeys: {},
  useProvidersQuery: () => ({
    data: { providers: {}, currentProviderId: "" },
    isLoading: false,
    refetch: vi.fn(),
  }),
  useSettingsQuery: () => ({
    data: {
      visibleApps: {
        claude: true,
        "claude-desktop": true,
        codex: true,
        gemini: true,
        grokbuild: true,
        opencode: true,
        openclaw: true,
        hermes: true,
      },
      useAppWindowControls: false,
      showProfileSwitcher: false,
      enableLocalProxy: false,
      enableFailoverToggle: false,
    },
  }),
}));

vi.mock("@/hooks/useProviderActions", () => ({
  useProviderActions: () => ({
    addProvider: vi.fn(),
    updateProvider: vi.fn(),
    switchProvider: vi.fn(),
    deleteProvider: vi.fn(),
    saveUsageScript: vi.fn(),
    setAsDefaultModel: vi.fn(),
  }),
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: false,
    takeoverStatus: {},
    status: { active_targets: [] },
  }),
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  openclawKeys: { liveProviderIds: ["openclawLiveProviderIds"] },
  useOpenClawHealth: () => ({ data: [] }),
}));

vi.mock("@/hooks/useHermes", () => ({
  hermesKeys: { liveProviderIds: ["hermesLiveProviderIds"] },
  useOpenHermesWebUI: () => vi.fn(),
}));

vi.mock("@/hooks/useUsageCacheBridge", () => ({
  useUsageCacheBridge: () => undefined,
}));
vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: () => undefined,
}));
vi.mock("@/lib/query/omo", () => ({
  useDisableCurrentOmo: () => ({ mutate: vi.fn() }),
  useDisableCurrentOmoSlim: () => ({ mutate: vi.fn() }),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    onSwitched: vi.fn().mockResolvedValue(() => undefined),
    updateTrayMenu: vi.fn(),
    getOpenCodeLiveProviderIds: vi.fn().mockResolvedValue([]),
    getOpenClawLiveProviderIds: vi.fn().mockResolvedValue([]),
    getHermesLiveProviderIds: vi.fn().mockResolvedValue([]),
    removeFromLiveConfig: vi.fn(),
    updateSortOrder: vi.fn(),
    openTerminal: vi.fn(),
  },
  settingsApi: { openExternal: vi.fn(), pickDirectory: vi.fn() },
}));
vi.mock("@/lib/api/env", () => ({
  checkAllEnvConflicts: vi.fn().mockResolvedValue({}),
  checkEnvConflicts: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/lib/api/hermes", () => ({
  hermesApi: { launchDashboard: vi.fn() },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    toggleMaximize: vi.fn(),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => undefined),
    setDecorations: vi.fn(),
    minimize: vi.fn(),
    close: vi.fn(),
  }),
}));

vi.mock("@/components/skills/LibrarySkillsPanel", async () => {
  const React = await import("react");
  return {
    LibrarySkillsPanel: React.forwardRef(
      ({ onOpenGlobal }: { onOpenGlobal?: () => void }, _ref) =>
        React.createElement(
          "div",
          { "data-testid": "library-view" },
          onOpenGlobal &&
            React.createElement(
              "button",
              { type: "button", onClick: onOpenGlobal },
              "open-global-from-library",
            ),
        ),
    ),
  };
});
vi.mock("@/components/skills/GlobalSkillsPanel", async () => {
  const React = await import("react");
  return {
    GlobalSkillsPanel: ({
      onOpenLibrary,
      onOpenProjects,
    }: {
      onOpenLibrary?: () => void;
      onOpenProjects?: () => void;
    }) =>
      React.createElement(
        "div",
        { "data-testid": "global-view" },
        React.createElement(
          "button",
          { type: "button", onClick: onOpenLibrary },
          "open-library-from-global",
        ),
        React.createElement(
          "button",
          { type: "button", onClick: onOpenProjects },
          "open-projects-from-global",
        ),
      ),
  };
});
vi.mock("@/components/skills/ProjectWorkspacesPanel", async () => {
  const React = await import("react");
  return {
    ProjectWorkspacesPanel: ({ onOpenGlobal }: { onOpenGlobal?: () => void }) =>
      React.createElement(
        "div",
        { "data-testid": "projects-view" },
        React.createElement(
          "button",
          { type: "button", onClick: onOpenGlobal },
          "open-global-from-projects",
        ),
      ),
  };
});
vi.mock("@/components/skills/SkillsPage", () => ({
  SkillsPage: () => <div data-testid="discovery-view" />,
  getSkillsPageHeaderActions: () => [],
}));

vi.mock("@/components/AppSwitcher", () => ({ AppSwitcher: Empty }));
vi.mock("@/components/profiles/ProfileSwitcher", () => ({
  ProfileSwitcher: Empty,
}));
vi.mock("@/components/providers/ProviderList", () => ({ ProviderList: Empty }));
vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: Empty,
}));
vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: Empty,
}));
vi.mock("@/components/ConfirmDialog", () => ({ ConfirmDialog: Empty }));
vi.mock("@/components/settings/SettingsPage", () => ({ SettingsPage: Empty }));
vi.mock("@/components/UpdateBadge", () => ({ UpdateBadge: Empty }));
vi.mock("@/components/env/EnvWarningBanner", () => ({
  EnvWarningBanner: Empty,
}));
vi.mock("@/components/proxy/ProxyToggle", () => ({ ProxyToggle: Empty }));
vi.mock("@/components/proxy/ClaudeDesktopRouteToggle", () => ({
  ClaudeDesktopRouteToggle: Empty,
}));
vi.mock("@/components/proxy/FailoverToggle", () => ({ FailoverToggle: Empty }));
vi.mock("@/components/UsageScriptModal", () => ({ default: Empty }));
vi.mock("@/components/mcp/UnifiedMcpPanel", () => ({ default: Empty }));
vi.mock("@/components/prompts/PromptPanel", () => ({ default: Empty }));
vi.mock("@/components/DeepLinkImportDialog", () => ({
  DeepLinkImportDialog: Empty,
}));
vi.mock("@/components/FirstRunNoticeDialog", () => ({
  FirstRunNoticeDialog: Empty,
}));
vi.mock("@/components/agents/AgentsPanel", () => ({ AgentsPanel: Empty }));
vi.mock("@/components/universal", () => ({ UniversalProviderPanel: Empty }));
vi.mock("@/components/sessions/SessionManagerPage", () => ({
  SessionManagerPage: Empty,
}));
vi.mock("@/components/workspace/WorkspaceFilesPanel", () => ({
  default: Empty,
}));
vi.mock("@/components/openclaw/EnvPanel", () => ({ default: Empty }));
vi.mock("@/components/openclaw/ToolsPanel", () => ({ default: Empty }));
vi.mock("@/components/openclaw/AgentsDefaultsPanel", () => ({
  default: Empty,
}));
vi.mock("@/components/openclaw/OpenClawHealthBanner", () => ({
  default: Empty,
}));
vi.mock("@/components/hermes/HermesMemoryPanel", () => ({ default: Empty }));
vi.mock("@/components/BrandIcons", () => ({ McpIcon: Empty }));

describe("App Skills navigation", () => {
  beforeEach(() => {
    localStorage.setItem("cc-switch-last-view", "skills");
    localStorage.setItem("cc-switch-last-app", "claude");
  });

  it("navigates Library → Global → Projects and back with view titles", async () => {
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <App />
      </QueryClientProvider>,
    );

    expect(screen.getByTestId("library-view")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "skills.library.title" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.global.title" }),
    );
    expect(await screen.findByTestId("global-view")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "skills.global.title" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "open-projects-from-global" }),
    );
    expect(await screen.findByTestId("projects-view")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "skills.projects.title" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "open-global-from-projects" }),
    );
    expect(await screen.findByTestId("global-view")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "common.back" }));
    expect(await screen.findByTestId("library-view")).toBeInTheDocument();
  });
});
