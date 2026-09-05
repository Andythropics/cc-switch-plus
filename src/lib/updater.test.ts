import { beforeEach, describe, expect, it, vi } from "vitest";
import { getVersion } from "@tauri-apps/api/app";
import {
  APP_UPDATE_POLICY,
  RELEASES_URL,
  automaticAppUpdatesEnabled,
} from "./product";
import { checkForUpdate } from "./updater";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(),
}));

describe("Plus app update policy", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps automatic app updates disabled", async () => {
    expect(APP_UPDATE_POLICY).toBe("manual");
    expect(automaticAppUpdatesEnabled()).toBe(false);
    expect(RELEASES_URL).toBe(
      "https://github.com/Andythropics/cc-switch-plus/releases",
    );
    await expect(checkForUpdate()).resolves.toEqual({ status: "manual" });
    expect(getVersion).not.toHaveBeenCalled();
  });
});
