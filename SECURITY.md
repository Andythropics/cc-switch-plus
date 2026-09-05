# Security Policy / 安全策略

Report vulnerabilities privately using [GitHub private vulnerability reporting](https://github.com/Andythropics/cc-switch-plus/security/advisories/new). Include the affected Plus version, platform, reproduction steps, impact, and a minimal redacted example. Do not include real API keys or personal configuration archives in public issues.

请通过 [GitHub 私密漏洞报告](https://github.com/Andythropics/cc-switch-plus/security/advisories/new)提交安全问题，包含版本、平台、复现步骤、影响和脱敏示例。请勿在公开 Issue 中上传真实 API 密钥或个人配置归档。

## Supported versions / 支持范围

The latest published Plus preview is the current maintenance target. Older Plus previews and upstream releases are not maintained here. This is a community project; no response-time SLA is promised.

本仓库维护最新发布的 Plus 预览版，不维护旧预览版或上游发行版。项目由社区维护，不承诺固定响应时限。

## Scope / 范围

CC Switch Plus runs with the current user's permissions and manages local configuration, credentials, Skill files, and symbolic links. It also inherits an optional local HTTP proxy. Remote repositories, ZIP files, sync payloads, deep links, and proxy requests can contain untrusted input.

重点关注仓库 / ZIP / 同步导入、深链接、代理请求中的不可信输入，以及它们对文件、凭据和受管理链接的影响。

Relevant reports include path traversal, unintended writes or deletes, credential disclosure, unsafe archive extraction, frontend injection, and unauthorized proxy behavior. Structural Skill validation is not a sandbox or a review of what an AI tool will do with those instructions.

有效报告包括路径穿越、越权写入或删除、凭据泄露、不安全的归档解压、前端注入与未授权代理行为。Skill 结构验证不等于沙箱，也不保证下游 AI 执行这些指令时的行为安全。

## Practical boundaries / 使用边界

- Back up `~/.cc-switch/` before migration; it may contain credentials. Store backups privately.
- Plus and original CC Switch share application data. Do not run both concurrently.
- Review unfamiliar Skills before deployment. Edits through a managed link affect the shared Library Skill.
- Keep the proxy on loopback unless you understand the network exposure and have configured suitable access controls.
- Configured providers, source repositories, pricing catalogs, and optional sync services receive requests when their features are used. This is a local desktop app, not an offline-only guarantee.
- Preview binaries are not Apple Developer signed or notarized. Check release checksums. Automatic upstream application updates are disabled; install Plus updates from this repository.

迁移前备份，并妥善保管可能含密钥的备份；勿同时运行两个共用数据的版本。部署前审查 Skill，注意共享链接的修改会影响所有部署。网络供应商、仓库、价格目录和可选同步服务仍会产生对应请求。预览包尚未完成 Apple 开发者签名和公证，请核验校验和并从本仓库手动更新。

## Known dependency advisories / 已知依赖问题

The inherited Linux GTK dependency graph includes `glib` 0.18.5, affected by [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g) (`VariantStrIter` memory unsoundness). This crate is absent from both macOS target dependency graphs and from the published macOS binaries. The preview does not publish or support Linux binaries. Linux source builds still need an upstream-compatible GTK dependency fix before they can be considered supported.

继承的 Linux GTK 依赖中仍包含受上述漏洞影响的 `glib` 0.18.5。该依赖不进入 Apple Silicon 或 Intel macOS 构建；本预览版不发布或支持 Linux 安装包。Linux 源码构建仍需等待兼容上游 GTK 依赖链的修复，不能视为已获支持的平台。

The Tauri build toolchain also retains `rand` 0.7.3 through `tauri-utils` → `kuchikiki` → `selectors` → `phf_codegen`. Its version is flagged by [GHSA-cq8v-f236-94qc](https://github.com/advisories/GHSA-cq8v-f236-94qc), but the advisory requires the crate's `log` feature, which is disabled in the resolved dependency graph. It is a build dependency, with no normal runtime dependency path in the macOS application. There is no compatible patched 0.7 release; this transitive dependency still requires an upstream update. The `rand` 0.8 and 0.9 lockfile entries have been updated to patched versions.

Tauri 构建工具链还间接保留了版本落入上述告警范围的 `rand` 0.7.3，但漏洞所需的 `log` 特性在当前依赖图中未启用。它仅用于构建，不进入 macOS 应用的常规运行时依赖链；0.7 系列没有兼容的修复版，仍需上游更新间接依赖。锁定文件中的 `rand` 0.8 和 0.9 已更新至修复版本。
