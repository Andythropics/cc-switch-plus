# Installation and migration / 安装与迁移

## Download / 下载

Use only [CC Switch Plus releases](https://github.com/Andythropics/cc-switch-plus/releases). The first preview targets macOS 12+ with separate Apple Silicon (`aarch64`) and Intel (`x86_64`) packages. Upstream CC Switch packages do not contain this fork's redesigned Skills system.

首版支持 macOS 12+，Apple Silicon 选择 `aarch64`，Intel 选择 `x86_64`。上游安装包不包含本分支的新版 Skill 管理。

Download a DMG or ZIP and `SHA256SUMS` from the same release. In the download directory:

```bash
shasum -a 256 CC-Switch-Plus-*.dmg
# Or / 或:
shasum -a 256 CC-Switch-Plus-*.zip
```

Compare the result for your file with its entry in `SHA256SUMS`. If you downloaded every asset, `shasum -a 256 -c SHA256SUMS` checks all of them.

将结果与 `SHA256SUMS` 中对应文件的值比较。下载了全部文件时，也可使用上述批量校验命令。

Open the DMG and move **CC Switch Plus.app** into Applications, or extract the ZIP and move the app there. Preview builds are not signed with an Apple Developer certificate or notarized. After verifying the source and checksum, use **System Settings → Privacy & Security → Open Anyway** if macOS blocks launch. Do not disable Gatekeeper globally.

打开 DMG 或解压 ZIP，将应用移入「应用程序」。预览版未使用 Apple 开发者证书签名或公证；核验来源及校验和后，若被系统拦截，可在「系统设置 → 隐私与安全性 → 仍要打开」中处理。

## Existing CC Switch users / 已有用户

1. In the original CC Switch, disable launch at login. Check macOS System Settings → General → Login Items and remove any remaining original CC Switch entry. Then quit both apps, including their tray instances.
2. Copy the entire `~/.cc-switch/` directory to a private backup location using Finder. Include the database, Skill Library, settings, and backups. If you changed the application data location, back up that directory instead.
3. Launch Plus and open Skills. Review the migration inventory, proposed actions, preserved content, and backup status.
4. Apply the reviewed plan, or defer and browse in read-only mode.
5. Inspect Claude / Codex deployments before continuing work.

先关闭原版的开机自启，并在 macOS「通用 → 登录项」确认没有残留原版入口；再退出两个应用及托盘实例，用 Finder 完整备份数据目录，再启动 Plus 审查迁移。更改过数据位置时请备份实际目录。可以应用已审查计划，也可以暂缓并只读浏览。

The bundle ID is distinct, but both applications intentionally use the same data directory. Do not run them concurrently. A newer database schema may be unreadable by an older upstream version. Do not simply install an older app over migrated data.

两个应用的标识不同，但共用数据目录，勿同时运行。新数据库结构可能不被旧版本支持；不要直接让旧应用打开已迁移的数据。

## Recovery / 恢复

Use the migration report's **Restore** action while its verified backup remains valid. If a conflict blocks recovery, preserve the current files and read the reported reason before changing anything. For a complete rollback, quit the app and restore your full pre-upgrade backup; keep the current directory as a separate backup until recovery is verified.

优先使用迁移报告中的「恢复」，前提是备份验证仍然有效。遇到冲突时先保留现有文件并阅读原因。完整回退时，退出应用并恢复升级前的完整备份；确认恢复成功前另存当前目录。

## Updates / 更新

Download new versions from this repository's releases. In-app upstream automatic updates are disabled so this fork cannot be replaced by the original distribution. Signed Plus auto-updates require a future release configuration.

更新请前往本仓库发布页手动下载。已关闭上游自动更新，避免增强分支被原版替换。未来自动更新需要配置 Plus 自己的签名与发布通道。

Custom data-directory settings are read from the original application path store, so existing overrides are preserved despite the new bundle identity. Launch-at-login registrations are separate: Plus does not silently remove the original application’s login item. Disable it as described above before migrating.

自定义数据目录继续从原有应用路径配置中读取，不因新应用标识丢失。登录项相互独立，Plus 不会静默删除原版登录项；迁移前请按上述步骤关闭。
