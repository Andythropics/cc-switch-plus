export const PRODUCT_NAME = "CC Switch Plus";

export const REPOSITORY_URL = "https://github.com/Andythropics/cc-switch-plus";
export const ISSUES_URL = `${REPOSITORY_URL}/issues`;
export const RELEASES_URL = `${REPOSITORY_URL}/releases`;

// The initial Plus prerelease is unsigned. App updates stay manual until the
// fork has its own signing key and release feed.
export const APP_UPDATE_POLICY = "manual" as const;
export const APP_UPDATES_ENABLED = false;

export const automaticAppUpdatesEnabled = (): boolean => APP_UPDATES_ENABLED;
