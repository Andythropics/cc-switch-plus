import { getVersion } from "@tauri-apps/api/app";
import { APP_UPDATE_POLICY } from "./product";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

export async function checkForUpdate(
  _opts: CheckOptions = {},
): Promise<
  | { status: "manual" }
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo }
> {
  // Plus does not have a signing key or update feed yet. Keep this guard in
  // the shared updater boundary so no caller can accidentally query upstream.
  return { status: APP_UPDATE_POLICY };
}
