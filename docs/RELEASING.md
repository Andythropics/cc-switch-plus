# Releasing CC Switch Plus / 发布流程

The initial channel is a macOS preview. Release tags use `v<version>`, for example `v3.20.1-plus.1`. The same version must appear in package.json, Cargo.toml, Cargo.lock, and tauri.conf.json.

首发使用 macOS 预览通道，标签和各版本文件必须保持一致。

1. Update the four version locations and [CHANGELOG_PLUS.md](../CHANGELOG_PLUS.md).
2. Run the [contributor checks](../CONTRIBUTING.md) and review [installation notes](INSTALL.md).
3. Push a version tag. The release workflow runs reusable CI, builds both Mac architectures, and creates a **draft prerelease** with DMG, ZIP, and SHA256SUMS assets.
4. Inspect both bundles, version / architecture / application identity, checksums, and workflow results. Smoke-test the available hardware and state any untested hardware honestly.
5. Edit the release notes for the version, then publish the draft as a prerelease.

先同步版本并验证，推送标签后工作流会检查、构建并创建草稿预发布。检查两个架构的安装包、版本、校验和与说明，完成可用硬件上的验证，再发布草稿。

The workflow can also be dispatched on an existing version tag to recover a failed run. It refuses a version mismatch. It does not publish a stable release automatically or create updater manifests.

工作流也可以在已有版本标签上手动运行，用于失败后重试。版本不匹配会终止；不会自动发布稳定版或生成自动更新清单。

## Signing / 签名

Preview packages are not Apple Developer signed or notarized. No upstream signing key, CDN, sponsor account, or release credential is reused. Before enabling automatic updates, establish a Plus-owned signing key, update endpoints, verification tests, and a documented recovery procedure.

预览包不使用 Apple 开发者证书签名或公证，也不复用上游密钥、CDN 或账号。开启自动更新前需配置自己的签名、端点、验证测试和恢复流程。

## Provenance / 来源

Retain the MIT notice, NOTICE, and Git history. Record the upstream commit incorporated into each Plus release. Do not present upstream downloads, rankings, funding links, or signatures as Plus's own.
