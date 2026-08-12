import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";
import { skillsApi } from "@/lib/api/skills";
import {
  getSettings,
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviders,
  setSettings,
} from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const skillsPanelMocks = vi.hoisted(() => ({
  openAcquireFromZip: vi.fn(),
  openDiscovery: vi.fn(),
}));
const platformState = vi.hoisted(() => ({ mac: true }));

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: () => platformState.mac,
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onCreate?.()}>create</button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/skills/LibrarySkillsPanel", async () => {
  const React = await import("react");
  const MockLibrarySkillsPanel = React.forwardRef((_props: any, ref) => {
    React.useImperativeHandle(ref, () => ({
      openDiscovery: skillsPanelMocks.openDiscovery,
      openAcquireFromZip: skillsPanelMocks.openAcquireFromZip,
    }));
    return <div data-testid="library-skills-panel" />;
  });
  MockLibrarySkillsPanel.displayName = "MockLibrarySkillsPanel";
  return { LibrarySkillsPanel: MockLibrarySkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

describe("App integration with MSW", () => {
  beforeEach(() => {
    resetProviderState();
    setSettings({ ...getSettings(), firstRunNoticeConfirmed: true });
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    skillsPanelMocks.openAcquireFromZip.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    platformState.mac = true;
    localStorage.removeItem("cc-switch-last-view");
  });

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("usage"));
    expect(screen.getByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByText("save-script"));
    fireEvent.click(screen.getByText("close-usage"));

    fireEvent.click(screen.getByText("create"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-add"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /New codex Provider/,
      ),
    );

    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-edit"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /-edited/,
      ),
    );

    fireEvent.click(screen.getByText("switch"));
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(/copy/),
    );

    fireEvent.click(screen.getByText("open-website"));

    emitTauriEvent("provider-switched", {
      appType: "codex",
      providerId: "codex-2",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalled();
  }, 10_000);

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("duplicates openclaw providers with a generated key that avoids live-only ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    setLiveProviderIds("openclaw", ["deepseek-copy"]);

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      const providerList = screen.getByTestId("provider-list").textContent;
      expect(providerList).toContain("deepseek-copy-2");
      expect(providerList).toContain("DeepSeek copy");
    });

    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for openclaw"),
    );
  });

  it("shows toast when duplicate cannot load live provider ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");

    const liveIdsSpy = vi
      .spyOn(providersApi, "getOpenClawLiveProviderIds")
      .mockRejectedValueOnce(new Error("broken config"));

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("读取配置中的供应商标识失败"),
      );
    });

    expect(screen.getByTestId("provider-list").textContent).not.toContain(
      "deepseek-copy",
    );

    liveIdsSpy.mockRestore();
  });

  it("hosts the Library ZIP acquisition action in the App toolbar", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("library-skills-panel"),
    ).toBeInTheDocument();
    const acquireZipButton = await screen.findByRole("button", {
      name: "skills.library.acquireZip",
    });
    await waitFor(() => expect(acquireZipButton).toBeEnabled());

    fireEvent.click(acquireZipButton);
    expect(skillsPanelMocks.openAcquireFromZip).toHaveBeenCalledTimes(1);
  });

  it("routes the Skills discover toolbar action through App navigation", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("library-skills-panel"),
    ).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "skills.discover",
      }),
    );

    expect(
      await screen.findByRole("heading", {
        name: "skills.library.discoveryTitle",
      }),
    ).toBeInTheDocument();
    expect(skillsPanelMocks.openDiscovery).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.queryByTestId("library-skills-panel"),
      ).not.toBeInTheDocument(),
    );
  });

  it("keeps Skills navigation visible and explains the macOS boundary without Library operations", async () => {
    platformState.mac = false;
    const preflightSpy = vi.spyOn(skillsApi, "inspectSkillsMigrationPreflight");
    const { default: App } = await import("@/App");
    renderApp(App);

    const skillsNavigation = await screen.findByTitle("skills.manage");
    fireEvent.click(skillsNavigation);

    expect(
      await screen.findByText("skills.library.macOnlyTitle"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("library-skills-panel"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "skills.library.acquireZip" }),
    ).not.toBeInTheDocument();
    expect(preflightSpy).not.toHaveBeenCalled();
    preflightSpy.mockRestore();
  });
});
