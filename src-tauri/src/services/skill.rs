//! Skill discovery, Library acquisition, and persisted migration inputs.
//! Active snapshots and deployments are owned by the Library services.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
#[cfg(not(target_os = "macos"))]
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::time::timeout;

#[cfg(debug_assertions)]
thread_local! {
    static LIBRARY_HASH_TRACE: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

use crate::app_config::AppType;
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::error::{format_skill_error, AppError};
#[cfg(target_os = "macos")]
use crate::services::activity::{
    ActivityActor, ActivityBatchContext, ActivityDetailCode, ActivityEventInput, ActivityOperation,
    ActivityOutcome, ActivityReason, ActivityRecorder, ActivityTarget, ActivityTrigger,
};

// ========== Legacy Skills state coordination (non-macOS) ==========

/// Coordinates the legacy database `skills` state with its filesystem SSOT.
///
/// macOS uses the redesigned Library and Deployment locks instead. Keeping this
/// lock behind the platform boundary preserves the upstream rollback model on
/// other platforms without reintroducing legacy SSOT mutations on macOS.
#[cfg(not(target_os = "macos"))]
fn skill_state_lock() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn skill_state_read_guard() -> RwLockReadGuard<'static, ()> {
    skill_state_lock().read().unwrap_or_else(|poisoned| {
        log::warn!("Skills state read lock was poisoned; recovering the protected state");
        poisoned.into_inner()
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn skill_state_write_guard() -> RwLockWriteGuard<'static, ()> {
    skill_state_lock().write().unwrap_or_else(|poisoned| {
        log::warn!("Skills state write lock was poisoned; recovering the protected state");
        poisoned.into_inner()
    })
}

// ========== 数据结构 ==========

/// Skill 同步方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncMethod {
    /// 自动选择：优先 symlink，失败时回退到 copy
    #[default]
    Auto,
    /// 符号链接（推荐，节省磁盘空间）
    Symlink,
    /// 文件复制（兼容模式）
    Copy,
}

/// Skill 存储位置（SSOT 目录选择）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillStorageLocation {
    /// CC Switch 管理目录 (~/.cc-switch/skills/)
    #[default]
    CcSwitch,
    /// Agent Skills 统一标准目录 (~/.agents/skills/)
    Unified,
}

/// 可发现的技能（来自仓库）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverableSkill {
    /// 唯一标识: "owner/name:directory"
    pub key: String,
    /// 显示名称 (从 SKILL.md 解析)
    pub name: String,
    /// 技能描述
    pub description: String,
    /// 目录名称 (安装路径的最后一段)
    pub directory: String,
    /// GitHub README URL
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    /// 仓库所有者
    #[serde(rename = "repoOwner")]
    pub repo_owner: String,
    /// 仓库名称
    #[serde(rename = "repoName")]
    pub repo_name: String,
    /// 分支名称
    #[serde(rename = "repoBranch")]
    pub repo_branch: String,
}

/// 仓库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRepo {
    /// GitHub 用户/组织名
    pub owner: String,
    /// 仓库名称
    pub name: String,
    /// 分支 (默认 "main")
    pub branch: String,
    /// 是否启用
    pub enabled: bool,
}

/// 持久化存储结构（仓库配置）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    /// 仓库列表
    pub repos: Vec<SkillRepo>,
}

impl Default for SkillStore {
    fn default() -> Self {
        SkillStore {
            repos: vec![
                SkillRepo {
                    owner: "anthropics".to_string(),
                    name: "skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "ComposioHQ".to_string(),
                    name: "awesome-claude-skills".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "cexll".to_string(),
                    name: "myclaude".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "JimLiu".to_string(),
                    name: "baoyu-skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                },
            ],
        }
    }
}

// ========== skills.sh API 类型 ==========

/// skills.sh API 原始响应
///
/// 注意：API 命名不一致（searchType 是 camelCase，duration_ms 是 snake_case），
/// 因此不能用 rename_all，需要逐字段指定。
#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiResponse {
    pub query: String,
    #[serde(rename = "searchType")]
    #[allow(dead_code)]
    pub search_type: String,
    pub skills: Vec<SkillsShApiSkill>,
    pub count: usize,
    #[allow(dead_code)]
    pub duration_ms: u64,
}

/// skills.sh API 原始技能条目
#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiSkill {
    pub id: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    pub installs: u64,
    pub source: String,
}

/// skills.sh 搜索结果（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShSearchResult {
    pub skills: Vec<SkillsShDiscoverableSkill>,
    pub total_count: usize,
    pub query: String,
}

/// skills.sh 可安装技能（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShDiscoverableSkill {
    pub key: String,
    pub name: String,
    pub directory: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub repo_branch: String,
    pub installs: u64,
    pub readme_url: Option<String>,
}

/// 仓库归档解压上限：条目数与解压后总字节数。
///
/// 归档字节由第三方完全控制（仓库可经 deeplink 添加，且 branch 可把下载落点
/// 改写到攻击者自传的 release asset），没有上限时一个几 MB 的压缩炸弹就能塞满磁盘。
/// 取值对齐 `webdav_sync/archive.rs` 里同款保护的量级。
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// symlink 目标就是一条路径，几十字节就够；给到 4 KiB 是宽松上限。
/// 必须有这个上限：zip 2.4.2 的 `make_reader` 不按声明的 uncompressed_size
/// 截断读取，所以一个打了 symlink 标志、deflate 流却能膨胀到数 GB 的条目，
/// 会被 `read_to_string` 整个读进内存。
const MAX_SYMLINK_TARGET_BYTES: u64 = 4 * 1024;
/// 物化一个目录按一个目录块计费。空目录不写内容字节，但照样吃 inode 和磁盘块，
/// 不计费就等于允许无限量地造目录。
const DIRECTORY_BUDGET_COST: u64 = 4096;
/// 压缩体上限。解压预算只有在 ZipArchive 建起来之后才生效，而那时整个响应体
/// 已经在内存里了，所以下载这一步需要自己的上限。技能仓库是 Markdown，
/// 128 MiB 的压缩包已经远超正常规模。
const MAX_ARCHIVE_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;

/// 技能元数据 (从 SKILL.md 解析)
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Origin retained for a Library Skill. Local ZIP acquisition deliberately has
/// no live path: after admission the private Library owns the snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySourceKind {
    Git,
    Zip,
    Marketplace,
    /// A local Skill imported as a Library-owned snapshot. It deliberately
    /// carries no path or update origin after admission.
    LocalImport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillSource {
    pub kind: LibrarySourceKind,
    pub url: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub repo_branch: Option<String>,
    pub skill_path: Option<String>,
    pub marketplace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerCompatibility {
    pub compatible: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkillCompatibility {
    pub claude: ConsumerCompatibility,
    pub codex: ConsumerCompatibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySkill {
    pub id: String,
    /// Immutable direct-child directory identity in ~/.cc-switch/skills.
    pub directory: String,
    /// User-editable presentation metadata; independent from `directory`.
    pub display_name: String,
    pub description: Option<String>,
    pub source: LibrarySkillSource,
    pub compatibility: LibrarySkillCompatibility,
    pub content_hash: String,
    pub acquired_at: i64,
    pub updated_at: i64,
}

// ========== SkillService ==========

pub struct SkillService;

impl Default for SkillService {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillService {
    pub fn new() -> Self {
        Self
    }

    /// 构建 Skill 文档 URL（指向仓库中的 SKILL.md 文件）
    ///
    /// 坐标不合法时返回 None：这个值会存进 `readme_url`，前端「查看文档」用
    /// `openExternal` 直接打开，恶意 branch 能把它指到 github.com 上的任意路径。
    fn build_skill_doc_url(
        owner: &str,
        repo: &str,
        branch: &str,
        doc_path: &str,
    ) -> Option<String> {
        if Self::validate_repo_ref(owner, repo, branch).is_err() {
            log::warn!("跳过非法仓库坐标的文档链接: {owner}/{repo}@{branch}");
            return None;
        }
        Some(format!(
            "https://github.com/{owner}/{repo}/blob/{branch}/{doc_path}"
        ))
    }

    /// 获取应用的 skills 目录
    pub fn get_app_skills_dir(app: &AppType) -> Result<PathBuf> {
        // 目录覆盖：优先使用用户在 settings.json 中配置的 override 目录
        match app {
            AppType::Claude => {
                if let Some(custom) = crate::settings::get_claude_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::ClaudeDesktop => {}
            AppType::Codex => {
                if let Some(custom) = crate::settings::get_codex_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Gemini => {
                if let Some(custom) = crate::settings::get_gemini_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::GrokBuild => {
                if let Some(custom) = crate::settings::get_grok_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::OpenCode => {
                if let Some(custom) = crate::settings::get_opencode_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::OpenClaw => {
                if let Some(custom) = crate::settings::get_openclaw_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Hermes => {
                if let Some(custom) = crate::settings::get_hermes_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Pi => {
                #[cfg(target_os = "macos")]
                return Err(anyhow!(
                    "Pi is not yet a redesigned Skill consumer or Deployment target"
                ));
                #[cfg(not(target_os = "macos"))]
                return Ok(crate::pi_config::get_pi_agent_dir()?.join("skills"));
            }
        }

        // 默认路径：回退到用户主目录下的标准位置。
        // 必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）：Windows 上 dirs::home_dir()
        // 走 Known Folder API，测试无法隔离真实用户目录。
        let home = crate::config::get_home_dir();

        Ok(match app {
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::ClaudeDesktop => home.join(".claude-desktop").join("skills"),
            AppType::Codex => home.join(".codex").join("skills"),
            AppType::Gemini => home.join(".gemini").join("skills"),
            AppType::GrokBuild => home.join(".grok").join("skills"),
            AppType::OpenCode => home.join(".config").join("opencode").join("skills"),
            AppType::OpenClaw => home.join(".openclaw").join("skills"),
            AppType::Hermes => crate::hermes_config::get_hermes_dir().join("skills"),
            AppType::Pi => {
                #[cfg(target_os = "macos")]
                return Err(anyhow!(
                    "Pi is not yet a redesigned Skill consumer or Deployment target"
                ));
                #[cfg(not(target_os = "macos"))]
                crate::pi_config::get_pi_agent_dir()?.join("skills")
            }
        })
    }

    // ========== 发现功能（保留原有逻辑）==========

    /// 列出所有可发现的技能（从仓库获取）
    pub async fn discover_available(
        &self,
        repos: Vec<SkillRepo>,
    ) -> Result<Vec<DiscoverableSkill>> {
        let mut skills = Vec::new();

        // 仅使用启用的仓库
        let enabled_repos: Vec<SkillRepo> = repos.into_iter().filter(|repo| repo.enabled).collect();

        let fetch_tasks = enabled_repos
            .iter()
            .map(|repo| self.fetch_repo_skills(repo));

        let results: Vec<Result<Vec<DiscoverableSkill>>> =
            futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results) {
            match result {
                Ok(repo_skills) => skills.extend(repo_skills),
                Err(e) => log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e),
            }
        }

        // 去重并排序
        Self::deduplicate_discoverable_skills(&mut skills);
        skills.sort_by_key(|skill| skill.name.to_lowercase());

        Ok(skills)
    }

    /// 从仓库获取技能列表
    async fn fetch_repo_skills(&self, repo: &SkillRepo) -> Result<Vec<DiscoverableSkill>> {
        let (temp_guard, resolved_branch) =
            timeout(std::time::Duration::from_secs(60), self.download_repo(repo))
                .await
                .map_err(|_| {
                    anyhow!(format_skill_error(
                        "DOWNLOAD_TIMEOUT",
                        &[
                            ("owner", &repo.owner),
                            ("name", &repo.name),
                            ("timeout", "60")
                        ],
                        Some("checkNetwork"),
                    ))
                })??;

        let mut skills = Vec::new();
        let scan_dir = temp_guard.path();
        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch;
        self.scan_dir_recursive(scan_dir, scan_dir, &resolved_repo, &mut skills)?;

        Ok(skills)
    }

    /// 递归扫描目录查找 SKILL.md
    fn scan_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &SkillRepo,
        skills: &mut Vec<DiscoverableSkill>,
    ) -> Result<()> {
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            let directory = if current_dir == base_dir {
                repo.name.clone()
            } else {
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .replace('\\', "/")
            };

            let doc_path = skill_md
                .strip_prefix(base_dir)
                .unwrap_or(skill_md.as_path())
                .to_string_lossy()
                .replace('\\', "/");

            if let Ok(skill) =
                self.build_skill_from_metadata(&skill_md, &directory, &doc_path, repo)
            {
                skills.push(skill);
            }

            return Ok(());
        }

        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir_recursive(&path, base_dir, repo, skills)?;
            }
        }

        Ok(())
    }

    /// 从 SKILL.md 构建技能对象
    fn build_skill_from_metadata(
        &self,
        skill_md: &Path,
        directory: &str,
        doc_path: &str,
        repo: &SkillRepo,
    ) -> Result<DiscoverableSkill> {
        let meta = self.parse_skill_metadata(skill_md)?;

        Ok(DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Self::build_skill_doc_url(&repo.owner, &repo.name, &repo.branch, doc_path),
            repo_owner: repo.owner.clone(),
            repo_name: repo.name.clone(),
            repo_branch: repo.branch.clone(),
        })
    }

    /// 解析技能元数据
    fn parse_skill_metadata(&self, path: &Path) -> Result<SkillMetadata> {
        Self::parse_skill_metadata_static(path)
    }

    /// 静态方法：解析技能元数据
    fn parse_skill_metadata_static(path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(path)?;
        let content = content.trim_start_matches('\u{feff}');

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Ok(SkillMetadata {
                name: None,
                description: None,
            });
        }

        let front_matter = parts[1].trim();
        let meta: SkillMetadata = serde_yaml::from_str(front_matter).unwrap_or(SkillMetadata {
            name: None,
            description: None,
        });

        Ok(meta)
    }

    /// 校验并规范化技能源路径（允许多级目录），拒绝路径穿越和绝对路径
    fn sanitize_skill_source_path(raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut normalized = PathBuf::new();
        let mut has_component = false;

        for component in Path::new(trimmed).components() {
            match component {
                Component::Normal(name) => {
                    let segment = name.to_string_lossy().trim().to_string();
                    if segment.is_empty() || segment == "." || segment == ".." {
                        return None;
                    }
                    normalized.push(segment);
                    has_component = true;
                }
                Component::CurDir
                | Component::ParentDir
                | Component::RootDir
                | Component::Prefix(_) => {
                    return None;
                }
            }
        }

        has_component.then_some(normalized)
    }

    /// 校验并规范化安装目录名（最终落盘目录名，仅单段）
    fn sanitize_install_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 显式拒绝两种分隔符，不能依赖 components() 的平台语义：
        // `\` 在 Linux/macOS 上不是分隔符，会被当成合法单段名放行，
        // 但同一个值同步/还原到 Windows 上就变成了嵌套路径。
        if trimmed.contains('/') || trimmed.contains('\\') {
            return None;
        }

        let path = Path::new(trimmed);
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => {
                let normalized = name.to_string_lossy().trim().to_string();
                if normalized.is_empty()
                    || normalized == "."
                    || normalized == ".."
                    || normalized.starts_with('.')
                {
                    None
                } else {
                    Some(normalized)
                }
            }
            _ => None,
        }
    }

    /// GitHub 账号名（user / org login）。
    ///
    /// 只放行 ASCII 字母数字与 `-`。这比 GitHub 自身的规则更严，但该字段会被拼进
    /// 下载 URL，任何 `/`、`.`、`%`、`\` 都可能改写请求落点（见 validate_repo_ref）。
    fn is_valid_github_owner(owner: &str) -> bool {
        !owner.is_empty()
            && owner.len() <= 39
            && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// GitHub 仓库名。允许 `.` `-` `_`，但整体不能是 `.` 或 `..`。
    fn is_valid_github_repo_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 100
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    }

    /// git 分支名。
    ///
    /// 分支名合法含 `/`（`feature/x`），所以不能整体禁掉分隔符——按段做白名单。
    /// 逐段 `!starts_with('.')` 比整体 `contains("..")` 更稳：它同时挡掉 `a/./b`、
    /// `a/.../b` 这类变形。除 `git check-ref-format` 的规则外还额外禁掉 `#` 与 `%`：
    /// 前者会把 URL 后半截变成 fragment，后者可用百分号编码绕过字符检查。
    fn is_valid_git_branch(branch: &str) -> bool {
        // 空串和 "HEAD" 都是 `download_repo` 的哨兵，语义都是「用仓库默认分支」：
        // 分支候选表对两者一视同仁地跳过，改试 main / master，所以它们**永远不会
        // 被拼进 URL**，也就没有可校验的攻击面。空串必须放行——`skill_repos` 的
        // 存量行可以是空 branch（建表默认值是 'main'，但不禁止空串），前端两处
        // `repo.branch || "main"` 就是照着这个前提写的。把它当非法会让那些仓库
        // 在 download_repo 第一行就报 INVALID_REPO_REF，技能面板直接列不出来。
        if branch.is_empty() || branch.eq_ignore_ascii_case("HEAD") {
            return true;
        }
        if branch.len() > 255 {
            return false;
        }
        if branch.starts_with('/') || branch.ends_with('/') || branch.contains("//") {
            return false;
        }
        if branch.contains("@{") {
            return false;
        }
        // `is_ascii_control()` 的范围是 U+0000..=U+001F **加上** U+007F DELETE，
        // 所以不需要另外再点名 DEL。
        if branch
            .chars()
            .any(|c| c.is_ascii_control() || " ~^:?*[\\#%".contains(c))
        {
            return false;
        }
        branch.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && !segment.ends_with('.')
                && !segment.ends_with(".lock")
        })
    }

    /// 校验一组仓库坐标，用于任何会被拼进 github.com URL 的地方。
    ///
    /// 动机：`download_repo` 把 owner/name/branch 直接 format 进
    /// `https://github.com/{owner}/{name}/archive/refs/heads/{branch}.zip`，而 URL
    /// 解析会消解点段——branch 写成 `../../../releases/download/v1/evil` 时，落点变成
    /// 该仓库的 **release asset**，即攻击者可上传的任意字节。归档内容一旦可控，
    /// 解压路径校验就成了唯一防线，所以这一层必须堵死。
    pub(crate) fn validate_repo_ref(owner: &str, name: &str, branch: &str) -> Result<()> {
        if !Self::is_valid_github_owner(owner) || !Self::is_valid_github_repo_name(name) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        if !Self::is_valid_git_branch(branch) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name), ("branch", branch)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 出口断言：URL 拼好后再确认它确实指向预期的 github.com 路径。
    ///
    /// 这是纵深防御——即便上面的字符集校验将来漏了某种变形（百分号编码、新的
    /// 分隔符语义等），这里也能拦住落点被改写的请求。
    fn assert_github_archive_url(url: &str, owner: &str, name: &str) -> Result<()> {
        let parsed = url::Url::parse(url).map_err(|e| anyhow!("Invalid archive URL: {e}"))?;
        let expected_prefix = format!("/{owner}/{name}/archive/refs/heads/");
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || !parsed.path().starts_with(&expected_prefix)
        {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 在目录树中查找名称匹配且包含 SKILL.md 的子目录
    ///
    /// 用于 skills.sh 安装回退：API 只返回 skillId（如 "find-skills"），
    /// 但实际文件可能在仓库子目录中（如 "skills/find-skills"）。
    fn find_skill_dir_by_name(root: &Path, target_name: &str) -> Option<PathBuf> {
        fn walk(dir: &Path, target: &str, depth: usize) -> Option<PathBuf> {
            if depth > 3 {
                return None;
            }
            let entries = fs::read_dir(dir).ok()?;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue;
                }
                if name_str.eq_ignore_ascii_case(target) && path.join("SKILL.md").exists() {
                    return Some(path);
                }
                if let Some(found) = walk(&path, target, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        walk(root, target_name, 0)
    }

    /// 将 discoverable skill 的目录信息重新解析为解压目录中的真实源目录。
    ///
    /// **核心原则：返回的目录必定含 `SKILL.md`**（以 SKILL.md 为锚点）。解析顺序：
    /// 1. 直接相对路径命中（如 `skills/foo`），校验含 `SKILL.md`——明确路径优先；
    /// 2. 按安装名递归查找名字匹配 **且** 含 `SKILL.md` 的目录；
    /// 3. 兜底：仓库根本身含 `SKILL.md`。
    fn resolve_skill_source_dir(root: &Path, raw_directory: &str) -> Option<PathBuf> {
        let source_rel = Self::sanitize_skill_source_path(raw_directory)?;
        let install_name = source_rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())?;

        // 1. 直接相对路径命中（明确路径优先）——必须校验 SKILL.md，否则同名空壳目录
        //    （如 ast-grep/agent-skill 根下的 plugin 包目录 ast-grep/）会被误判为源目录。
        let direct = root.join(&source_rel);
        if direct.is_dir() && direct.join("SKILL.md").is_file() {
            return Some(direct);
        }

        // 2. 按名字递归查找（find_skill_dir_by_name 已校验 SKILL.md）
        if let Some(found) = Self::find_skill_dir_by_name(root, &install_name) {
            log::info!(
                "Skill directory '{}' not found at direct path, using fallback: {}",
                install_name,
                found.display()
            );
            return Some(found);
        }

        // 3. 兜底：仓库根本身是 skill
        if root.join("SKILL.md").is_file() {
            log::info!(
                "Skill directory '{}' not found, but SKILL.md exists at root, using repo root",
                install_name,
            );
            return Some(root.to_path_buf());
        }

        None
    }

    /// 由真实解析出的源目录推导 SKILL.md 在仓库内的相对文档路径（正斜杠）。
    /// 两个参数都应是已 canonicalize 的路径（安装流程已做包含性校验）。
    fn doc_path_for_source(repo_root: &Path, source: &Path) -> Option<String> {
        let rel = source.strip_prefix(repo_root).ok()?;
        let mut parts: Vec<String> = rel
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        parts.push("SKILL.md".to_string());
        Some(parts.join("/"))
    }

    /// 去重技能列表（基于完整 key，不同仓库的同名 skill 分开显示）
    fn deduplicate_discoverable_skills(skills: &mut Vec<DiscoverableSkill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            // 使用完整 key（owner/repo:directory）作为唯一标识
            // 这样不同仓库的同名 skill 会分开显示
            let unique_key = skill.key.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(unique_key) {
                e.insert(true);
                true
            } else {
                false
            }
        });
    }

    /// 下载仓库
    ///
    /// 这里是仓库坐标进入 URL 的**唯一收敛点**——`fetch_repo_skills`、`install`、
    /// `check_updates`、`update_skill` 四条路径都经过它，而 `skill_repos` / `skills`
    /// 两张表都会被同步导入的远端快照整表覆盖，入库校验管不住它们。所以主防线放这里。
    async fn download_repo(&self, repo: &SkillRepo) -> Result<(tempfile::TempDir, String)> {
        Self::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;

        // 守卫全程持有，成功后连同目录一起交给调用方（见 `extract_local_zip` 的说明）。
        // 原来这里立刻 keep()，任何一步失败——下载超时、ARCHIVE_TOO_LARGE、解压出错
        // ——都会把半个解压目录永久留在磁盘上，反复触发即可持续填盘。
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();

        let mut branches = Vec::new();
        if !repo.branch.is_empty() && !repo.branch.eq_ignore_ascii_case("HEAD") {
            branches.push(repo.branch.as_str());
        }
        if !branches.contains(&"main") {
            branches.push("main");
        }
        if !branches.contains(&"master") {
            branches.push("master");
        }

        let mut last_error = None;
        for branch in branches {
            let url = format!(
                "https://github.com/{}/{}/archive/refs/heads/{}.zip",
                repo.owner, repo.name, branch
            );
            Self::assert_github_archive_url(&url, &repo.owner, &repo.name)?;

            match self.download_and_extract(&url, &temp_path).await {
                Ok(_) => return Ok((temp_dir, branch.to_string())),
                Err(e) => {
                    // 每个分支各自重算预算，所以失败后必须把上一轮的残留清掉——
                    // 否则 N 个候选分支等于 N 倍的落盘量堆在同一个目录里。
                    let _ = fs::remove_dir_all(&temp_path);
                    let _ = fs::create_dir_all(&temp_path);
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("所有分支下载失败")))
    }

    /// 下载并解压 ZIP
    async fn download_and_extract(&self, url: &str, dest: &Path) -> Result<()> {
        let client = crate::proxy::http_client::get();
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            let status = response.status().as_u16().to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "DOWNLOAD_FAILED",
                &[("status", &status)],
                match status.as_str() {
                    "403" => Some("http403"),
                    "404" => Some("http404"),
                    "429" => Some("http429"),
                    _ => Some("checkNetwork"),
                },
            )));
        }

        // 逐块读并卡住压缩体大小：`response.bytes()` 会先把攻击者控制的整个归档
        // 收进内存，之后才轮到 ZipArchive 和解压预算——那时候堆已经被吃光了。
        // 不能只信 Content-Length（可以撒谎或缺失），必须按实际收到的字节数算。
        let mut response = response;
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
                let limit_mb = (MAX_ARCHIVE_DOWNLOAD_BYTES / 1024 / 1024).to_string();
                return Err(anyhow::anyhow!(format_skill_error(
                    "ARCHIVE_TOO_LARGE",
                    &[("limit_mb", &limit_mb)],
                    Some("checkZipContent"),
                )));
            }
            body.extend_from_slice(&chunk);
        }

        let cursor = std::io::Cursor::new(body);
        let archive = zip::ZipArchive::new(cursor)?;
        Self::extract_repo_archive(archive, dest)
    }

    /// 按预算把单个归档条目写出，累计超限即中止。
    ///
    /// 逐块累加而非读取归档头里声明的 size：那个值由归档作者填写，压缩炸弹会撒谎。
    fn copy_entry_within_budget<R: std::io::Read, W: std::io::Write>(
        reader: &mut R,
        writer: &mut W,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            Self::charge_archive_budget(total_bytes, read as u64)?;
            writer.write_all(&buffer[..read])?;
        }
    }

    /// 读取 symlink 条目声明的目标路径。
    ///
    /// 这条分支曾是唯一一处不经预算的解压：`read_to_string` 直接把整条解压流吞进
    /// 内存，而 zip 2.4.2 的 `make_reader`（read.rs:437-449）只叠了 CRC 校验，
    /// **没有**按声明的 uncompressed_size 截断。于是一个打着 symlink 标志、
    /// deflate 后能膨胀到数 GB 的条目就是一颗内存炸弹，且预算读数全程为 0。
    ///
    /// 超长或非 UTF-8 一律返回 `None` 让调用方跳过：合法的 symlink 目标是一条
    /// 路径，这两种形状都不可能是真实数据。
    fn read_symlink_target<R: std::io::Read>(
        reader: &mut R,
        total_bytes: &mut u64,
    ) -> Result<Option<String>> {
        let mut raw = Vec::new();
        // 多读一个字节，用来区分"正好到上限"和"被截断"
        let mut limited = std::io::Read::take(reader, MAX_SYMLINK_TARGET_BYTES + 1);
        std::io::Read::read_to_end(&mut limited, &mut raw)?;
        if raw.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
            return Ok(None);
        }
        Self::charge_archive_budget(total_bytes, raw.len() as u64)?;
        Ok(String::from_utf8(raw)
            .ok()
            .map(|target| target.trim().to_string()))
    }

    /// 建目录并按**实际新建的层数**计费。
    ///
    /// `create_dir_all` 会一次性把缺失的父目录全建出来，所以一个条目名
    /// `a/a/…/a/f.txt` 可以隐式造出几百层目录。只在 symlink 物化那条路径上给目录
    /// 计费是不够的：常规解压这条路上，不到 10_000 个条目照样能造出数百万目录，
    /// 而内容字节几乎为零。
    fn create_dir_all_within_budget(path: &Path, total_bytes: &mut u64) -> Result<()> {
        let missing = path.ancestors().take_while(|p| !p.exists()).count() as u64;
        if missing > 0 {
            Self::charge_archive_budget(total_bytes, missing * DIRECTORY_BUDGET_COST)?;
        }
        fs::create_dir_all(path)?;
        Ok(())
    }

    /// 归档预算的唯一扣费点。
    ///
    /// 抽出来是因为「写文件内容」不是归档能消耗的唯一资源：symlink 物化出来的
    /// 目录一个字节都不写，但每一个都要占 inode 与一个目录块，而第二遍的
    /// symlink 解析可以让目录数量按层数指数增长。只按内容字节计费时，一个全是
    /// 空目录的归档能把预算读数一直停在 0。
    fn charge_archive_budget(total_bytes: &mut u64, amount: u64) -> Result<()> {
        if total_bytes.saturating_add(amount) > MAX_ARCHIVE_TOTAL_BYTES {
            let limit_mb = (MAX_ARCHIVE_TOTAL_BYTES / 1024 / 1024).to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_LARGE",
                &[("limit_mb", &limit_mb)],
                Some("checkZipContent"),
            )));
        }
        *total_bytes += amount;
        Ok(())
    }

    /// 把 GitHub 仓库归档解压到 `dest`（剥掉归档自带的一层根目录）。
    ///
    /// 与 `download_and_extract` 分离，使 zip-slip 防护可在不联网的情况下被单测覆盖。
    fn extract_repo_archive<R: std::io::Read + std::io::Seek>(
        mut archive: zip::ZipArchive<R>,
        dest: &Path,
    ) -> Result<()> {
        let root_name = if !archive.is_empty() {
            let first_file = archive.by_index(0)?;
            let name = first_file.name();
            name.split('/').next().unwrap_or("").to_string()
        } else {
            return Err(anyhow::anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkRepoUrl"),
            )));
        };

        // 归档字节完全由第三方控制（仓库可经 deeplink 添加），所以解压必须限量，
        // 否则一个几 MB 的压缩炸弹就能塞满磁盘。webdav_sync/archive.rs 早有同款
        // 双重上限，这条下载路径一直没有。
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }
        let mut total_bytes: u64 = 0;

        // 第一遍：解压普通文件和目录，收集 symlink 条目
        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            // 第一道：enclosed_name() 拒绝绝对路径、盘符前缀，以及净深度为负
            // （即逃出归档自身根目录）的条目。skill 仓库可由 deeplink 添加，
            // 压缩包内容属第三方可控输入。
            let Some(safe_path) = file.enclosed_name() else {
                log::warn!("跳过不安全的压缩包条目: {}", file.name());
                continue;
            };

            // GitHub 归档统一带一层 `<repo>-<branch>/` 根目录，需剥掉后再落盘。
            let Ok(relative_path) = safe_path.strip_prefix(&root_name) else {
                continue;
            };

            // 第二道：enclosed_name() 的保证是相对**归档根**的，且它不规范化路径
            // ——`..` 会原样留在返回值里。上面剥掉 root_name 等于花掉一级深度预算，
            // 于是 `repo-main/../evil` 这类条目仍能落到 dest 之外（Unix 逃一层；
            // Windows 上 root_name 可含反斜杠而被当作多段，逃逸深度随之放大）。
            // 因此 join 之前必须对**实际使用的相对路径**再验一次。
            if relative_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                log::warn!("跳过越界的压缩包条目: {}", file.name());
                continue;
            }

            if relative_path.as_os_str().is_empty() {
                continue;
            }

            let outpath = dest.join(relative_path);

            if file.is_symlink() {
                let Some(target) = Self::read_symlink_target(&mut file, &mut total_bytes)? else {
                    log::warn!("跳过目标不合法的 symlink 条目: {}", file.name());
                    continue;
                };
                symlinks.push((outpath, target));
            } else if file.is_dir() {
                Self::create_dir_all_within_budget(&outpath, &mut total_bytes)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    Self::create_dir_all_within_budget(parent, &mut total_bytes)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                // 按实际写入的字节累计，而不是信任归档头里声明的 size——
                // 压缩炸弹的声明值可以是假的。
                Self::copy_entry_within_budget(&mut file, &mut outfile, &mut total_bytes)?;
            }
        }

        // 第二遍：解析 symlink，将目标内容复制到 symlink 位置
        Self::resolve_symlinks_in_dir(dest, &symlinks, &mut total_bytes)?;

        Ok(())
    }

    /// 与 `copy_dir_recursive` 同语义，但把写出的字节计入归档总预算。
    /// 仅用于解压期间物化 symlink——常规的目录复制（安装、备份、迁移）不该受
    /// 归档预算约束，所以两个函数刻意不合并。
    fn copy_dir_within_budget(src: &Path, dest: &Path, total_bytes: &mut u64) -> Result<()> {
        Self::create_dir_all_within_budget(dest, total_bytes)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Self::copy_dir_within_budget(&path, &dest_path, total_bytes)?;
            } else {
                Self::copy_file_within_budget(&path, &dest_path, total_bytes)?;
            }
        }

        Ok(())
    }

    /// 复制单个文件并计入归档总预算，复用 `copy_entry_within_budget` 以保证
    /// 上限与报错文案只有一处定义。
    fn copy_file_within_budget(src: &Path, dest: &Path, total_bytes: &mut u64) -> Result<()> {
        let mut reader = fs::File::open(src)?;
        let mut writer = fs::File::create(dest)?;
        Self::copy_entry_within_budget(&mut reader, &mut writer, total_bytes)
    }

    /// 解析 ZIP 中的符号链接：将目标内容复制到 symlink 位置
    ///
    /// GitHub ZIP 归档保留了 symlink 元数据，解压时可通过 `is_symlink()` 检测。
    /// 此方法将 symlink 解析为实际文件/目录内容（而非创建真实 symlink），
    /// 以确保跨平台兼容且 skill 内容自包含。
    fn resolve_symlinks_in_dir(
        base_dir: &Path,
        symlinks: &[(PathBuf, String)],
        total_bytes: &mut u64,
    ) -> Result<()> {
        // 规范化 base_dir（macOS 上 /tmp → /private/tmp，需保持一致）
        let canonical_base = base_dir
            .canonicalize()
            .unwrap_or_else(|_| base_dir.to_path_buf());

        for (link_path, target) in symlinks {
            // 计算 symlink 的父目录，然后拼接目标的相对路径
            let parent = link_path.parent().unwrap_or(base_dir);
            let resolved = parent.join(target);

            // 规范化路径（解析 .. 等）
            let resolved = match resolved.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    log::warn!(
                        "Symlink 目标不存在，跳过: {} -> {}",
                        link_path.display(),
                        target
                    );
                    continue;
                }
            };

            // 安全检查一：确保目标在 base_dir 内（防止路径穿越）
            if !resolved.starts_with(&canonical_base) {
                log::warn!(
                    "Symlink 目标超出仓库范围，跳过: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 安全检查二：目标不能包含 link 自身。上面那条防的是「跑出 base」，
            // 防不住「套进自己」——`dir/link -> ..` 解析后正是 base 本身，完全
            // 合规，随后递归复制会把归档根复制进自己的子目录；每递归一层都重新
            // 看到刚落盘的副本，目录树逐层膨胀直到 PATH_MAX 才失败。
            //
            // 比较必须在**规范形式**上做：`enclosed_name()` 不规范化路径，只保证
            // 净深度非负，所以 link_path 里可能带着未消解的 `..`（`e/../d/self`）。
            // 拿它按字面跟 canonicalize 过的 resolved 比组件，第一段就会错开
            // （`e` vs `d`），检查形同虚设。link_path 自身此刻尚未落盘，但它的父
            // 目录一定存在——`resolved` 能 canonicalize 成功就蕴含了这一点。
            let canonical_link = match parent.canonicalize() {
                Ok(canonical_parent) => match link_path.file_name() {
                    Some(name) => canonical_parent.join(name),
                    None => canonical_parent,
                },
                // 父目录都不存在时退回字面形式：此时 resolved 多半也解析不出来，
                // 上面就已经 continue 了；留着只是不让守卫在意外形状上 panic。
                Err(_) => match link_path.strip_prefix(base_dir) {
                    Ok(relative) => canonical_base.join(relative),
                    Err(_) => link_path.clone(),
                },
            };
            if canonical_link.starts_with(&resolved) {
                log::warn!(
                    "Symlink 目标包含链接自身，跳过（会导致递归自复制）: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 复制目标内容到 symlink 位置。必须与解压循环共用同一个字节预算：
            // 物化走的是这条独立路径，不计费的话「一个大文件 + N 个指向它的
            // symlink」能写下 N 倍字节，而 MAX_ARCHIVE_TOTAL_BYTES 全程显示合规。
            if resolved.is_dir() {
                Self::copy_dir_within_budget(&resolved, link_path, total_bytes)?;
            } else if resolved.is_file() {
                if let Some(parent) = link_path.parent() {
                    Self::create_dir_all_within_budget(parent, total_bytes)?;
                }
                Self::copy_file_within_budget(&resolved, link_path, total_bytes)?;
            }
        }
        Ok(())
    }

    // ========== skills.sh 搜索 ==========

    /// 搜索 skills.sh 公共目录
    pub async fn search_skills_sh(
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SkillsShSearchResult> {
        let client = crate::proxy::http_client::get();

        let url = url::Url::parse_with_params(
            "https://skills.sh/api/search",
            &[
                ("q", query),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ],
        )?;

        let resp = client
            .get(url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json::<SkillsShApiResponse>()
            .await?;

        let skills = resp
            .skills
            .into_iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.source.splitn(2, '/').collect();
                if parts.len() != 2 {
                    return None;
                }
                let (owner, repo) = (parts[0].to_string(), parts[1].to_string());
                // 用与 download_repo 同一套坐标校验，而不是就地写启发式：下面这个
                // readme_url 最终交给 openExternal 打开，是和 build_skill_doc_url
                // 同一个 sink。原来的 `contains('.')` 既漏（`splitn(2, '/')` 允许
                // repo 里带 `/`，`owner/a/b` 能拼出三段路径），又误伤（GitHub 仓库
                // 名合法含点）。校验 owner 同时也保留了"过滤非 GitHub 来源"的效果
                // ——`skills.volces.com` 这类带点的 owner 本来就不是合法用户名。
                if Self::validate_repo_ref(&owner, &repo, "main").is_err() {
                    return None;
                }
                Some(SkillsShDiscoverableSkill {
                    key: s.id,
                    name: s.name,
                    directory: s.skill_id.clone(),
                    repo_owner: owner.clone(),
                    repo_name: repo.clone(),
                    repo_branch: "main".to_string(),
                    installs: s.installs,
                    readme_url: Some(format!("https://github.com/{}/{}", owner, repo)),
                })
            })
            .collect();

        Ok(SkillsShSearchResult {
            skills,
            total_count: resp.count,
            query: resp.query,
        })
    }
}

/// Admission boundary for the redesigned, macOS-only Skill Library.
///
/// This service owns Library validation and writes. It intentionally has no
/// dependency on application discovery paths or the legacy `enabled_*` flags,
/// so acquisition cannot accidentally become deployment.
pub struct LibrarySkillAcquisitionService;

/// Serializes all Library directory/metadata mutations, including local
/// imports. Composite project imports acquire this same lock before taking
/// the Deployment lock so acquisition and import cannot race a directory or
/// database admission decision.
static LIBRARY_MUTATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct LibrarySkillSourceInspection {
    pub display_name: String,
    pub description: Option<String>,
    pub compatibility: LibrarySkillCompatibility,
    pub content_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryAdmissionDisposition {
    Created,
    Reused,
}

#[derive(Debug, Clone)]
struct LibraryAdmission {
    skill: LibrarySkill,
    disposition: LibraryAdmissionDisposition,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct ZipBatchActivityRecorded {
    message: String,
}

impl LibrarySkillAcquisitionService {
    pub(crate) fn lock_for_composite() -> Result<MutexGuard<'static, ()>> {
        LIBRARY_MUTATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub(crate) fn ensure_supported_platform() -> Result<()> {
        if !cfg!(target_os = "macos") {
            return Err(anyhow!(
                "The redesigned Skill Library is supported on macOS only"
            ));
        }
        Ok(())
    }

    fn library_dir() -> Result<PathBuf> {
        let dir = Self::library_directory_path();
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Return the private Library directory without creating it. Read-only
    /// import inspection uses this path helper so a preview never mutates
    /// application or project state.
    pub(crate) fn library_directory_path() -> PathBuf {
        get_app_config_dir().join("skills")
    }

    /// Validate a local source and compute the same canonical content hash
    /// used by Library acquisition, without writing anything.
    pub(crate) fn inspect_source_directory(source: &Path) -> Result<LibrarySkillSourceInspection> {
        Self::ensure_supported_platform()?;
        let metadata = fs::symlink_metadata(source)
            .with_context(|| format!("failed to access Skill source {}", source.display()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "Skill source must be a real directory: {}",
                source.display()
            ));
        }
        Self::validate_internal_symlinks(source)?;
        let (display_name, description, compatibility) = Self::read_manifest(source)?;
        let content_hash = Self::compute_library_hash(source)?;
        Ok(LibrarySkillSourceInspection {
            display_name,
            description,
            compatibility,
            content_hash,
        })
    }

    fn compatibility_issue(message: impl Into<String>) -> ConsumerCompatibility {
        ConsumerCompatibility {
            compatible: false,
            issues: vec![message.into()],
        }
    }

    fn validate_consumer_metadata(
        metadata: Option<&serde_yaml::Mapping>,
        parse_issue: Option<&str>,
    ) -> ConsumerCompatibility {
        if let Some(issue) = parse_issue {
            return Self::compatibility_issue(issue);
        }
        let Some(metadata) = metadata else {
            return Self::compatibility_issue("SKILL.md must start with YAML front matter");
        };

        let string_value = |key: &str| {
            metadata
                .get(serde_yaml::Value::String(key.to_string()))
                .and_then(serde_yaml::Value::as_str)
                .map(str::trim)
        };

        let Some(name) = string_value("name").filter(|value| !value.is_empty()) else {
            return Self::compatibility_issue("front matter must contain a non-empty name");
        };
        if name.len() > 64
            || name.starts_with('-')
            || name.ends_with('-')
            || name.contains("--")
            || !name
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Self::compatibility_issue(
                "name must be 1-64 lowercase letters, digits, or single hyphens",
            );
        }

        let Some(description) = string_value("description").filter(|value| !value.is_empty())
        else {
            return Self::compatibility_issue("front matter must contain a non-empty description");
        };
        if description.chars().count() > 1024 {
            return Self::compatibility_issue("description must be at most 1024 characters");
        }

        ConsumerCompatibility {
            compatible: true,
            issues: Vec::new(),
        }
    }

    pub(crate) fn read_manifest(
        source: &Path,
    ) -> Result<(String, Option<String>, LibrarySkillCompatibility)> {
        let manifest = source.join("SKILL.md");
        if !manifest.is_file() {
            let compatibility = LibrarySkillCompatibility {
                claude: Self::compatibility_issue("canonical SKILL.md is missing"),
                codex: Self::compatibility_issue("canonical SKILL.md is missing"),
            };
            return Err(anyhow!(
                "Skill is incompatible with Claude and Codex: {:?}",
                compatibility
            ));
        }

        let content = fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let normalized = content.trim_start_matches('\u{feff}');
        let front_matter = (|| -> std::result::Result<&str, String> {
            let first_end = normalized
                .find('\n')
                .ok_or_else(|| "SKILL.md front matter is not closed".to_string())?;
            if normalized[..first_end].trim_end_matches('\r') != "---" {
                return Err("SKILL.md must start with YAML front matter".to_string());
            }
            let body_start = first_end + 1;
            let mut cursor = body_start;
            for line in normalized[body_start..].split_inclusive('\n') {
                let delimiter = line.strip_suffix('\n').unwrap_or(line);
                let delimiter = delimiter.strip_suffix('\r').unwrap_or(delimiter);
                if delimiter == "---" {
                    return Ok(&normalized[body_start..cursor]);
                }
                cursor += line.len();
            }
            Err("SKILL.md front matter is not closed".to_string())
        })();
        let (metadata, parse_issue) = match front_matter {
            Ok(front_matter) => match serde_yaml::from_str::<serde_yaml::Value>(front_matter) {
                Ok(serde_yaml::Value::Mapping(mapping)) => (Some(mapping), None),
                Ok(_) => (
                    None,
                    Some("YAML front matter must be a mapping".to_string()),
                ),
                Err(error) => (None, Some(format!("invalid YAML front matter: {error}"))),
            },
            Err(issue) => (None, Some(issue)),
        };

        // Claude Code and Codex currently share the canonical Agent Skills
        // name/description contract. They remain separate adapter results so a
        // future consumer-specific rule does not require rewriting source.
        let claude = Self::validate_consumer_metadata(metadata.as_ref(), parse_issue.as_deref());
        let codex = Self::validate_consumer_metadata(metadata.as_ref(), parse_issue.as_deref());
        let compatibility = LibrarySkillCompatibility { claude, codex };
        if !compatibility.claude.compatible && !compatibility.codex.compatible {
            return Err(anyhow!(
                "Skill is incompatible with Claude and Codex: {:?}",
                compatibility
            ));
        }

        let metadata = metadata.expect("compatible metadata must be a mapping");
        let name = metadata
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(serde_yaml::Value::as_str)
            .expect("compatible metadata must contain name")
            .trim()
            .to_string();
        let description = metadata
            .get(serde_yaml::Value::String("description".to_string()))
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        Ok((name, description, compatibility))
    }

    pub(crate) fn validate_internal_symlinks(root: &Path) -> Result<()> {
        let canonical_root = root
            .canonicalize()
            .with_context(|| format!("failed to resolve Skill root {}", root.display()))?;

        fn walk(current: &Path, canonical_root: &Path) -> Result<()> {
            for entry in fs::read_dir(current)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() {
                    let target = fs::read_link(&path)?;
                    if target.is_absolute() {
                        return Err(anyhow!(
                            "absolute internal symlink is not allowed: {} -> {}",
                            path.display(),
                            target.display()
                        ));
                    }
                    let resolved = path
                        .parent()
                        .unwrap_or(current)
                        .join(&target)
                        .canonicalize()
                        .with_context(|| {
                            format!(
                                "broken internal symlink is not allowed: {} -> {}",
                                path.display(),
                                target.display()
                            )
                        })?;
                    if !resolved.starts_with(canonical_root) {
                        return Err(anyhow!(
                            "escaping internal symlink is not allowed: {} -> {}",
                            path.display(),
                            target.display()
                        ));
                    }
                } else if metadata.is_dir() {
                    walk(&path, canonical_root)?;
                } else if !metadata.is_file() {
                    return Err(anyhow!(
                        "unsupported file type in Skill source: {}",
                        path.display()
                    ));
                }
            }
            Ok(())
        }

        walk(root, &canonical_root)
    }

    pub(crate) fn copy_tree_preserving_links(source: &Path, destination: &Path) -> Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path)?;
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&source_path)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &destination_path).with_context(|| {
                    format!(
                        "failed to preserve symlink {} -> {}",
                        destination_path.display(),
                        target.display()
                    )
                })?;
                #[cfg(not(unix))]
                return Err(anyhow!(
                    "preserving Skill symlinks is unsupported on this platform"
                ));
            } else if metadata.is_dir() {
                Self::copy_tree_preserving_links(&source_path, &destination_path)?;
                #[cfg(unix)]
                fs::set_permissions(
                    &destination_path,
                    fs::Permissions::from_mode(metadata.permissions().mode()),
                )?;
            } else if metadata.is_file() {
                fs::copy(&source_path, &destination_path)?;
                #[cfg(unix)]
                fs::set_permissions(
                    &destination_path,
                    fs::Permissions::from_mode(metadata.permissions().mode()),
                )?;
            } else {
                return Err(anyhow!(
                    "unsupported file type in Skill source: {}",
                    source_path.display()
                ));
            }
        }
        #[cfg(unix)]
        if let Ok(metadata) = fs::symlink_metadata(source) {
            fs::set_permissions(
                destination,
                fs::Permissions::from_mode(metadata.permissions().mode()),
            )?;
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn start_hash_trace_for_test() {
        LIBRARY_HASH_TRACE.with(|trace| *trace.borrow_mut() = Some(Vec::new()));
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn take_hash_trace_for_test() -> Vec<PathBuf> {
        LIBRARY_HASH_TRACE.with(|trace| trace.borrow_mut().take().unwrap_or_default())
    }

    /// Versioned, length-framed tree digest. Permissions include executable bits only.
    pub(crate) fn compute_library_hash(root: &Path) -> Result<String> {
        #[cfg(debug_assertions)]
        LIBRARY_HASH_TRACE.with(|trace| {
            if let Some(paths) = trace.borrow_mut().as_mut() {
                paths.push(root.to_path_buf());
            }
        });
        use sha2::{Digest, Sha256};
        fn field(hash: &mut Sha256, bytes: &[u8]) {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        fn walk(root: &Path, current: &Path, hash: &mut Sha256) -> Result<()> {
            let mut entries = fs::read_dir(current)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                field(
                    hash,
                    path.strip_prefix(root)?.as_os_str().as_encoded_bytes(),
                );
                if metadata.file_type().is_symlink() {
                    field(hash, b"link");
                    field(hash, fs::read_link(&path)?.as_os_str().as_encoded_bytes());
                } else {
                    field(hash, if metadata.is_dir() { b"dir" } else { b"file" });
                    #[cfg(unix)]
                    field(hash, &(metadata.permissions().mode() & 0o111).to_be_bytes());
                    #[cfg(not(unix))]
                    field(hash, &0u32.to_be_bytes());
                    if metadata.is_dir() {
                        walk(root, &path, hash)?;
                    } else if metadata.is_file() {
                        field(hash, &fs::read(&path)?);
                    } else {
                        return Err(anyhow!("unsupported Skill file type: {}", path.display()));
                    }
                }
            }
            Ok(())
        }
        let mut hash = Sha256::new();
        field(&mut hash, b"cc-switch-library-v2");
        walk(root, root, &mut hash)?;
        Ok(format!("v2:{:x}", hash.finalize()))
    }

    /// Legacy baselines stay immutable until the next explicit snapshot update.
    /// Recompute their original encoding to avoid marking every old row modified.
    pub(crate) fn hash_matches_baseline(path: &Path, recorded: &str) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(value) if value.is_dir() && !value.file_type().is_symlink() => (),
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let current = if recorded.starts_with("v2:") {
            Self::compute_library_hash(path)?
        } else {
            Self::compute_legacy_library_hash(path)?
        };
        Ok(current == recorded)
    }

    /// A recorded baseline is not proof of the contents currently deployed.
    pub(crate) fn find_identical_skill(
        db: &Arc<Database>,
        hash: &str,
    ) -> Result<Option<LibrarySkill>> {
        for skill in db.list_library_skills()? {
            let path = Self::library_directory_path().join(&skill.directory);
            if Self::inspect_source_directory(&path)
                .is_ok_and(|current| current.content_hash == hash)
            {
                return Ok(Some(skill));
            }
        }
        Ok(None)
    }

    pub(crate) fn compute_legacy_library_hash(root: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        fn collect(root: &Path, current: &Path, entries: &mut Vec<PathBuf>) -> Result<()> {
            for entry in fs::read_dir(current)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.is_dir() && !metadata.file_type().is_symlink() {
                    collect(root, &path, entries)?;
                } else {
                    entries.push(path.strip_prefix(root)?.to_path_buf());
                }
            }
            Ok(())
        }

        let mut entries = Vec::new();
        collect(root, root, &mut entries)?;
        entries.sort();
        let mut hasher = Sha256::new();
        for relative in entries {
            let path = root.join(&relative);
            hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
            hasher.update(b"\0");
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                hasher.update(b"link\0");
                hasher.update(fs::read_link(&path)?.to_string_lossy().as_bytes());
            } else {
                hasher.update(b"file\0");
                hasher.update(fs::read(&path)?);
            }
            hasher.update(b"\0");
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn read_library_symlink_target<R: std::io::Read>(
        reader: &mut R,
        total_bytes: &mut u64,
    ) -> Result<PathBuf> {
        let mut raw = Vec::new();
        let mut limited = std::io::Read::take(reader, MAX_SYMLINK_TARGET_BYTES + 1);
        std::io::Read::read_to_end(&mut limited, &mut raw)?;
        if raw.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
            return Err(anyhow!("symlink target in Skill ZIP is too long"));
        }
        SkillService::charge_archive_budget(total_bytes, raw.len() as u64)?;
        let target = String::from_utf8(raw)
            .map_err(|_| anyhow!("symlink target in Skill ZIP is not UTF-8"))?;
        if target.is_empty() || target.contains('\0') {
            return Err(anyhow!("symlink target in Skill ZIP is invalid"));
        }
        Ok(PathBuf::from(target))
    }

    /// Extract a user-selected archive without dereferencing its symlinks.
    /// Admission validates every link after extraction and before any Library
    /// destination is created.
    fn extract_zip_preserving_links(zip_path: &Path) -> Result<tempfile::TempDir> {
        let file = fs::File::open(zip_path)
            .with_context(|| format!("failed to open ZIP file: {}", zip_path.display()))?;
        let archive = zip::ZipArchive::new(file)
            .with_context(|| format!("failed to read ZIP file: {}", zip_path.display()))?;
        Self::extract_archive_preserving_links(archive)
    }

    fn extract_archive_preserving_links<R: std::io::Read + std::io::Seek>(
        mut archive: zip::ZipArchive<R>,
    ) -> Result<tempfile::TempDir> {
        if archive.is_empty() {
            return Err(anyhow!("Skill ZIP is empty"));
        }
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(anyhow!(
                "Skill ZIP has too many entries ({}; limit {})",
                archive.len(),
                MAX_ARCHIVE_ENTRIES
            ));
        }

        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path();
        let mut total_bytes = 0;
        let mut symlinks = Vec::new();

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| anyhow!("unsafe path in Skill ZIP: {}", entry.name()))?;
            if relative
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(anyhow!("unsafe path in Skill ZIP: {}", entry.name()));
            }
            let output = root.join(relative);
            if entry.is_symlink() {
                let target = Self::read_library_symlink_target(&mut entry, &mut total_bytes)?;
                if target.is_absolute() {
                    return Err(anyhow!(
                        "absolute internal symlink is not allowed: {} -> {}",
                        output.display(),
                        target.display()
                    ));
                }
                symlinks.push((output, target));
            } else if entry.is_dir() {
                SkillService::create_dir_all_within_budget(&output, &mut total_bytes)?;
            } else {
                if let Some(parent) = output.parent() {
                    SkillService::create_dir_all_within_budget(parent, &mut total_bytes)?;
                }
                let mut file = fs::File::create(&output)?;
                SkillService::copy_entry_within_budget(&mut entry, &mut file, &mut total_bytes)?;
                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
        }

        for (link, target) in symlinks {
            if let Some(parent) = link.parent() {
                SkillService::create_dir_all_within_budget(parent, &mut total_bytes)?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link).with_context(|| {
                format!(
                    "failed to preserve symlink {} -> {}",
                    link.display(),
                    target.display()
                )
            })?;
            #[cfg(not(unix))]
            return Err(anyhow!(
                "preserving Skill symlinks is unsupported on this platform"
            ));
        }
        Self::validate_internal_symlinks(root)?;
        Ok(temp_dir)
    }

    fn scan_library_skills(root: &Path) -> Result<Vec<PathBuf>> {
        fn walk(current: &Path, results: &mut Vec<PathBuf>) -> Result<()> {
            if current.join("SKILL.md").is_file() {
                results.push(current.to_path_buf());
                return Ok(());
            }
            for entry in fs::read_dir(current)? {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && !entry.file_name().to_string_lossy().starts_with('.')
                {
                    walk(&entry.path(), results)?;
                }
            }
            Ok(())
        }

        let mut results = Vec::new();
        walk(root, &mut results)?;
        results.sort();
        Ok(results)
    }

    async fn download_repo_preserving_links(
        repo: &SkillRepo,
    ) -> Result<(tempfile::TempDir, PathBuf, String)> {
        SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
        let mut branches = Vec::new();
        if !repo.branch.is_empty() && !repo.branch.eq_ignore_ascii_case("HEAD") {
            branches.push(repo.branch.as_str());
        }
        if !branches.contains(&"main") {
            branches.push("main");
        }
        if !branches.contains(&"master") {
            branches.push("master");
        }

        let mut last_error = None;
        for branch in branches {
            let result = Self::download_repository_branch(&repo.owner, &repo.name, branch).await;

            match result {
                Ok((extracted, repo_root)) => {
                    return Ok((extracted, repo_root, branch.to_string()));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("all GitHub branch downloads failed")))
    }

    /// Download exactly the persisted upstream branch for a Library update.
    /// Unlike discovery acquisition this helper never falls back to main or
    /// master: a deleted/moved branch must become an invalid candidate rather
    /// than silently changing the Library's origin.
    #[cfg(target_os = "macos")]
    pub(crate) async fn download_repository_snapshot_exact(
        source: &LibrarySkillSource,
    ) -> Result<(tempfile::TempDir, PathBuf)> {
        let owner = source
            .repo_owner
            .as_deref()
            .ok_or_else(|| anyhow!("Library Skill source has no repository owner"))?;
        let repo = source
            .repo_name
            .as_deref()
            .ok_or_else(|| anyhow!("Library Skill source has no repository name"))?;
        let branch = source
            .repo_branch
            .as_deref()
            .ok_or_else(|| anyhow!("Library Skill source has no persisted repository branch"))?;
        Self::download_repository_branch(owner, repo, branch).await
    }

    async fn download_repository_branch(
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<(tempfile::TempDir, PathBuf)> {
        SkillService::validate_repo_ref(owner, repo, branch)?;
        let url = format!("https://github.com/{owner}/{repo}/archive/refs/heads/{branch}.zip");
        SkillService::assert_github_archive_url(&url, owner, repo)?;
        let response = crate::proxy::http_client::get().get(&url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "GitHub archive download failed with status {}",
                response.status()
            ));
        }
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
                return Err(anyhow!("GitHub archive exceeds the download limit"));
            }
            body.extend_from_slice(&chunk);
        }
        let archive = zip::ZipArchive::new(std::io::Cursor::new(body))?;
        let extracted = Self::extract_archive_preserving_links(archive)?;
        let mut entries = fs::read_dir(extracted.path())?
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(fs::DirEntry::file_name);
        let repo_root = if entries.len() == 1 {
            let entry = &entries[0];
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                entry.path()
            } else {
                extracted.path().to_path_buf()
            }
        } else {
            extracted.path().to_path_buf()
        };
        Ok((extracted, repo_root))
    }

    pub async fn acquire_discoverable(
        db: &Arc<Database>,
        skill: &DiscoverableSkill,
        source_kind: LibrarySourceKind,
        requested_directory: Option<&str>,
    ) -> Result<LibrarySkill> {
        let result = async {
            Self::ensure_supported_platform()?;
            if !matches!(
                source_kind,
                LibrarySourceKind::Git | LibrarySourceKind::Marketplace
            ) {
                return Err(anyhow!(
                    "this source kind must use the local Skill Import endpoint"
                ));
            }
            let repo = SkillRepo {
                owner: skill.repo_owner.clone(),
                name: skill.repo_name.clone(),
                branch: skill.repo_branch.clone(),
                enabled: true,
            };
            let (_extracted, repo_root, resolved_branch) =
                Self::download_repo_preserving_links(&repo).await?;
            Self::acquire_from_repository_snapshot_inner(
                db,
                &repo_root,
                skill,
                source_kind,
                &resolved_branch,
                requested_directory,
            )
        }
        .await;
        Self::record_admission_result(db, &result);
        result.map(|admission| admission.skill)
    }

    /// Admit a downloaded repository snapshot. Keeping network transfer above
    /// this seam lets Git and marketplace acquisition share real-filesystem
    /// validation tests without mocking HTTP.
    pub fn acquire_from_repository_snapshot(
        db: &Arc<Database>,
        repo_root: &Path,
        skill: &DiscoverableSkill,
        source_kind: LibrarySourceKind,
        resolved_branch: &str,
        requested_directory: Option<&str>,
    ) -> Result<LibrarySkill> {
        let result = Self::acquire_from_repository_snapshot_inner(
            db,
            repo_root,
            skill,
            source_kind,
            resolved_branch,
            requested_directory,
        );
        Self::record_admission_result(db, &result);
        result.map(|admission| admission.skill)
    }

    fn acquire_from_repository_snapshot_inner(
        db: &Arc<Database>,
        repo_root: &Path,
        skill: &DiscoverableSkill,
        source_kind: LibrarySourceKind,
        resolved_branch: &str,
        requested_directory: Option<&str>,
    ) -> Result<LibraryAdmission> {
        Self::ensure_supported_platform()?;
        if !matches!(
            source_kind,
            LibrarySourceKind::Git | LibrarySourceKind::Marketplace
        ) {
            return Err(anyhow!(
                "this source kind must use the local Skill Import endpoint"
            ));
        }
        SkillService::validate_repo_ref(&skill.repo_owner, &skill.repo_name, resolved_branch)?;
        let source = SkillService::resolve_skill_source_dir(repo_root, &skill.directory)
            .ok_or_else(|| {
                anyhow!(
                    "canonical SKILL.md was not found for '{}' in {}/{}",
                    skill.directory,
                    skill.repo_owner,
                    skill.repo_name
                )
            })?;
        let canonical_root = repo_root.canonicalize()?;
        let canonical_source = source.canonicalize()?;
        if !canonical_source.starts_with(&canonical_root) {
            return Err(anyhow!("resolved Skill source escapes its Git repository"));
        }
        let doc_path = SkillService::doc_path_for_source(&canonical_root, &canonical_source);
        let skill_path = doc_path
            .as_deref()
            .and_then(|path| path.strip_suffix("/SKILL.md"))
            .filter(|path| !path.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (doc_path.as_deref() == Some("SKILL.md")).then(|| ".".to_string()));
        // Discovery metadata may name a branch that was unavailable and fell
        // back during download. Persist the source URL from the snapshot that
        // was actually admitted, not the stale requested URL.
        let url = SkillService::build_skill_doc_url(
            &skill.repo_owner,
            &skill.repo_name,
            resolved_branch,
            doc_path.as_deref().unwrap_or("SKILL.md"),
        );
        let default_directory = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("Skill needs a readable Library directory name"))?;
        let _guard = Self::lock_for_composite()?;
        Self::acquire_from_directory_locked_with_disposition(
            db,
            &source,
            LibrarySkillSource {
                kind: source_kind,
                url,
                repo_owner: Some(skill.repo_owner.clone()),
                repo_name: Some(skill.repo_name.clone()),
                repo_branch: Some(resolved_branch.to_string()),
                skill_path,
                marketplace: (source_kind == LibrarySourceKind::Marketplace)
                    .then(|| "skills.sh".to_string()),
            },
            requested_directory.or(Some(default_directory.as_str())),
        )
    }

    pub fn acquire_from_zip(
        db: &Arc<Database>,
        zip_path: &Path,
        requested_directories: &HashMap<String, String>,
    ) -> Result<Vec<LibrarySkill>> {
        let result = Self::acquire_from_zip_inner(db, zip_path, requested_directories);
        if let Err(error) = &result {
            if error.downcast_ref::<ZipBatchActivityRecorded>().is_none() {
                Self::record_library_activity(
                    db,
                    ActivityReason::Acquire,
                    ActivityOutcome::Failed,
                    Self::activity_detail_for_error(error),
                    None,
                    None,
                );
            }
        }
        result
    }

    fn acquire_from_zip_inner(
        db: &Arc<Database>,
        zip_path: &Path,
        requested_directories: &HashMap<String, String>,
    ) -> Result<Vec<LibrarySkill>> {
        Self::ensure_supported_platform()?;
        let extracted = Self::extract_zip_preserving_links(zip_path)?;
        let skill_dirs = Self::scan_library_skills(extracted.path())?;
        if skill_dirs.is_empty() {
            return Err(anyhow!("Skill ZIP does not contain a canonical SKILL.md"));
        }
        let _guard = Self::lock_for_composite()?;

        let mut candidates = Vec::with_capacity(skill_dirs.len());
        for source in skill_dirs {
            let relative = source
                .strip_prefix(extracted.path())?
                .to_string_lossy()
                .replace('\\', "/");
            let default_directory = if source == extracted.path() {
                zip_path
                    .file_stem()
                    .map(|name| name.to_string_lossy().to_string())
            } else {
                source
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
            }
            .ok_or_else(|| anyhow!("Skill needs a readable Library directory name"))?;
            let requested = requested_directories
                .get(&relative)
                .or_else(|| requested_directories.get(&default_directory))
                .cloned()
                .unwrap_or(default_directory);
            candidates.push((source, relative, requested));
        }

        // A multi-Skill ZIP is admitted as one request. Validate every source
        // and every directory collision before writing the first snapshot so
        // a retry cannot collide with an earlier partial success.
        let mut batch_directories = HashSet::new();
        let mut batch_hashes = HashSet::new();
        for (source, _relative, requested) in &candidates {
            let metadata = fs::symlink_metadata(source).map_err(|error| {
                anyhow!("read Skill source metadata {}: {error}", source.display())
            })?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(anyhow!(
                    "Skill source is not a real directory: {}",
                    source.display()
                ));
            }
            Self::validate_internal_symlinks(source)?;
            Self::read_manifest(source)?;
            let hash = Self::compute_library_hash(source)?;
            if Self::find_identical_skill(db, &hash)?.is_some() || !batch_hashes.insert(hash) {
                continue;
            }
            let directory = Self::require_available_directory(db, requested)?;
            if !batch_directories.insert(directory.to_lowercase()) {
                return Err(anyhow!(
                    "LIBRARY_DIRECTORY_CONFLICT: '{directory}' is repeated in this ZIP; choose readable unique names"
                ));
            }
        }

        let item_count = candidates.len() as u32;
        let mut acquired: Vec<LibraryAdmission> = Vec::with_capacity(candidates.len());
        for (failed_index, (source, relative, requested)) in candidates.iter().enumerate() {
            let result = Self::acquire_from_directory_locked_with_disposition(
                db,
                source,
                LibrarySkillSource {
                    kind: LibrarySourceKind::Zip,
                    url: None,
                    repo_owner: None,
                    repo_name: None,
                    repo_branch: None,
                    skill_path: if relative.is_empty() {
                        None
                    } else {
                        Some(relative.clone())
                    },
                    marketplace: None,
                },
                Some(requested),
            );
            match result {
                Ok(admission) => acquired.push(admission),
                Err(error) => {
                    let mut compensation_errors = Vec::new();
                    let mut compensation_failed = vec![false; acquired.len()];
                    for (index, admission) in acquired.iter().enumerate() {
                        if admission.disposition == LibraryAdmissionDisposition::Reused {
                            continue;
                        }
                        let skill = &admission.skill;
                        let path = Self::library_directory_path().join(&skill.directory);
                        let rollback = Self::library_directory_path()
                            .join(format!(".zip-rollback-{}", uuid::Uuid::new_v4()));
                        if let Err(compensation) = fs::rename(&path, &rollback) {
                            compensation_failed[index] = true;
                            compensation_errors.push(compensation.to_string());
                            continue;
                        }
                        if let Err(compensation) = db.delete_library_skill(&skill.id) {
                            let restore = fs::rename(&rollback, &path).err();
                            compensation_failed[index] = true;
                            compensation_errors.push(match restore {
                                Some(restore) => {
                                    format!("{compensation}; restore failed: {restore}")
                                }
                                None => compensation.to_string(),
                            });
                        } else if let Err(compensation) = fs::remove_dir_all(&rollback) {
                            compensation_failed[index] = true;
                            compensation_errors.push(compensation.to_string());
                        }
                    }

                    let batch_id = uuid::Uuid::new_v4().to_string();
                    for (index, admission) in acquired.iter().enumerate() {
                        let (outcome, detail_code) = match admission.disposition {
                            LibraryAdmissionDisposition::Reused => {
                                (ActivityOutcome::NoOp, ActivityDetailCode::AlreadyInSync)
                            }
                            LibraryAdmissionDisposition::Created if compensation_failed[index] => (
                                ActivityOutcome::CompensationFailed,
                                ActivityDetailCode::CompensationFailure,
                            ),
                            LibraryAdmissionDisposition::Created => {
                                (ActivityOutcome::RolledBack, ActivityDetailCode::None)
                            }
                        };
                        Self::record_library_activity(
                            db,
                            ActivityReason::Acquire,
                            outcome,
                            detail_code,
                            Some(admission.skill.id.clone()),
                            Some(ActivityBatchContext {
                                batch_id: batch_id.clone(),
                                item_index: index as u32,
                                item_count,
                            }),
                        );
                    }
                    Self::record_library_activity(
                        db,
                        ActivityReason::Acquire,
                        ActivityOutcome::Failed,
                        Self::activity_detail_for_error(&error),
                        None,
                        Some(ActivityBatchContext {
                            batch_id: batch_id.clone(),
                            item_index: failed_index as u32,
                            item_count,
                        }),
                    );
                    for index in (failed_index + 1)..candidates.len() {
                        Self::record_library_activity(
                            db,
                            ActivityReason::Acquire,
                            ActivityOutcome::Blocked,
                            ActivityDetailCode::PartialBatch,
                            None,
                            Some(ActivityBatchContext {
                                batch_id: batch_id.clone(),
                                item_index: index as u32,
                                item_count,
                            }),
                        );
                    }
                    let message = if compensation_errors.is_empty() {
                        error.to_string()
                    } else {
                        format!(
                            "ZIP Library admission failed ({error}); batch compensation failed: {}",
                            compensation_errors.join("; ")
                        )
                    };
                    return Err(ZipBatchActivityRecorded { message }.into());
                }
            }
        }
        let batch_id = (item_count > 1).then(|| uuid::Uuid::new_v4().to_string());
        for (index, admission) in acquired.iter().enumerate() {
            Self::record_library_activity(
                db,
                ActivityReason::Acquire,
                match admission.disposition {
                    LibraryAdmissionDisposition::Created => ActivityOutcome::Success,
                    LibraryAdmissionDisposition::Reused => ActivityOutcome::NoOp,
                },
                match admission.disposition {
                    LibraryAdmissionDisposition::Created => ActivityDetailCode::None,
                    LibraryAdmissionDisposition::Reused => ActivityDetailCode::AlreadyInSync,
                },
                Some(admission.skill.id.clone()),
                batch_id.as_ref().map(|batch_id| ActivityBatchContext {
                    batch_id: batch_id.clone(),
                    item_index: index as u32,
                    item_count,
                }),
            );
        }
        Ok(acquired
            .into_iter()
            .map(|admission| admission.skill)
            .collect())
    }

    fn require_available_directory(db: &Arc<Database>, raw_directory: &str) -> Result<String> {
        let directory = SkillService::sanitize_install_name(raw_directory).ok_or_else(|| {
            anyhow!("Library directory name must be one readable path segment: {raw_directory:?}")
        })?;
        if directory != raw_directory.trim() {
            return Err(anyhow!(
                "Library directory name must be supplied in canonical form: {raw_directory:?}"
            ));
        }
        let destination = Self::library_dir()?.join(&directory);
        if db.get_library_skill_by_directory(&directory)?.is_some()
            || fs::symlink_metadata(&destination).is_ok()
        {
            return Err(anyhow!(
                "LIBRARY_DIRECTORY_CONFLICT: '{directory}' is already in use; choose a readable unique name"
            ));
        }
        Ok(directory)
    }

    pub fn acquire_from_directory(
        db: &Arc<Database>,
        source: &Path,
        upstream: LibrarySkillSource,
        requested_directory: Option<&str>,
    ) -> Result<LibrarySkill> {
        let _guard = Self::lock_for_composite()?;
        let result = Self::acquire_from_directory_locked_with_disposition(
            db,
            source,
            upstream,
            requested_directory,
        );
        match result {
            Ok(admission) => {
                Self::record_library_activity(
                    db,
                    ActivityReason::Acquire,
                    match admission.disposition {
                        LibraryAdmissionDisposition::Created => ActivityOutcome::Success,
                        LibraryAdmissionDisposition::Reused => ActivityOutcome::NoOp,
                    },
                    match admission.disposition {
                        LibraryAdmissionDisposition::Created => ActivityDetailCode::None,
                        LibraryAdmissionDisposition::Reused => ActivityDetailCode::AlreadyInSync,
                    },
                    Some(admission.skill.id.clone()),
                    None,
                );
                Ok(admission.skill)
            }
            Err(error) => {
                Self::record_library_activity(
                    db,
                    ActivityReason::Acquire,
                    ActivityOutcome::Failed,
                    Self::activity_detail_for_error(&error),
                    None,
                    None,
                );
                Err(error)
            }
        }
    }

    /// Admission primitive for a caller already holding the shared Library
    /// mutation lock (for example project import-and-replace). Keeping this
    /// separate avoids recursively locking the non-reentrant mutex while the
    /// caller coordinates a Deployment transaction.
    pub(crate) fn acquire_from_directory_locked(
        db: &Arc<Database>,
        source: &Path,
        upstream: LibrarySkillSource,
        requested_directory: Option<&str>,
    ) -> Result<LibrarySkill> {
        Self::acquire_from_directory_locked_with_disposition(
            db,
            source,
            upstream,
            requested_directory,
        )
        .map(|admission| admission.skill)
    }

    /// Admit a legacy Skill that is already physically located at its final
    /// private Library path. Migration uses this instead of copying a directory
    /// onto itself; normal acquisition never needs this seam.
    pub(crate) fn admit_existing_library_directory_locked(
        db: &Arc<Database>,
        directory: &str,
    ) -> Result<LibrarySkill> {
        Self::ensure_supported_platform()?;
        let canonical = SkillService::sanitize_install_name(directory)
            .filter(|value| value == directory)
            .ok_or_else(|| anyhow!("Library directory identity is invalid"))?;
        if let Some(existing) = db.get_library_skill_by_directory(&canonical)? {
            let path = Self::library_directory_path().join(&canonical);
            if !Self::hash_matches_baseline(&path, &existing.content_hash)? {
                return Err(anyhow!("Library Skill content drifted during migration"));
            }
            return Ok(existing);
        }
        let path = Self::library_directory_path().join(&canonical);
        let inspection = Self::inspect_source_directory(&path)?;
        if Self::find_identical_skill(db, &inspection.content_hash)?.is_some() {
            return Err(anyhow!(
                "Library content already belongs to a different directory identity"
            ));
        }
        let now = chrono::Utc::now().timestamp();
        let skill = LibrarySkill {
            id: uuid::Uuid::new_v4().to_string(),
            directory: canonical,
            display_name: inspection.display_name,
            description: inspection.description,
            source: LibrarySkillSource {
                kind: LibrarySourceKind::LocalImport,
                url: None,
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                skill_path: None,
                marketplace: None,
            },
            compatibility: inspection.compatibility,
            content_hash: inspection.content_hash,
            acquired_at: now,
            updated_at: now,
        };
        db.save_library_skill(&skill)?;
        Ok(skill)
    }

    fn acquire_from_directory_locked_with_disposition(
        db: &Arc<Database>,
        source: &Path,
        upstream: LibrarySkillSource,
        requested_directory: Option<&str>,
    ) -> Result<LibraryAdmission> {
        Self::ensure_supported_platform()?;
        let source_metadata = fs::symlink_metadata(source)
            .with_context(|| format!("read Skill source metadata: {}", source.display()))?;
        if !source_metadata.is_dir() || source_metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "Skill source must be a real directory: {}",
                source.display()
            ));
        }
        Self::validate_internal_symlinks(source)?;
        let (display_name, description, compatibility) = Self::read_manifest(source)?;
        let source_content_hash = Self::compute_library_hash(source)?;
        if let Some(existing) = Self::find_identical_skill(db, &source_content_hash)? {
            return Ok(LibraryAdmission {
                skill: existing,
                disposition: LibraryAdmissionDisposition::Reused,
            });
        }

        let raw_directory = requested_directory
            .map(ToOwned::to_owned)
            .or_else(|| {
                source
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .ok_or_else(|| anyhow!("Skill needs a readable Library directory name"))?;
        let directory = Self::require_available_directory(db, &raw_directory)?;

        let library_dir = Self::library_dir()?;
        let destination = library_dir.join(&directory);

        let id = uuid::Uuid::new_v4().to_string();
        let staging = library_dir.join(format!(".acquiring-{id}"));
        let copy_result = (|| -> Result<()> {
            Self::copy_tree_preserving_links(source, &staging)?;
            Self::validate_internal_symlinks(&staging)?;
            fs::rename(&staging, &destination).with_context(|| {
                format!(
                    "failed to admit Skill into Library: {} -> {}",
                    staging.display(),
                    destination.display()
                )
            })?;
            Ok(())
        })();
        if let Err(error) = copy_result {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        let content_hash = match Self::compute_library_hash(&destination) {
            Ok(hash) => hash,
            Err(error) => {
                let _ = fs::remove_dir_all(&destination);
                return Err(error);
            }
        };
        if content_hash != source_content_hash {
            let _ = fs::remove_dir_all(&destination);
            return Err(anyhow!(
                "admitted Skill content changed while copying into the Library"
            ));
        }
        let acquired_at = Utc::now().timestamp();
        let skill = LibrarySkill {
            id,
            directory,
            display_name,
            description,
            source: upstream,
            compatibility,
            content_hash,
            acquired_at,
            updated_at: acquired_at,
        };
        if let Err(error) = db.save_library_skill(&skill) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error.into());
        }
        Ok(LibraryAdmission {
            skill,
            disposition: LibraryAdmissionDisposition::Created,
        })
    }

    pub fn update_display_metadata(
        db: &Arc<Database>,
        id: &str,
        display_name: &str,
        description: Option<&str>,
    ) -> Result<LibrarySkill> {
        let _guard = Self::lock_for_composite()?;
        let result = (|| {
            Self::ensure_supported_platform()?;
            let display_name = display_name.trim();
            if display_name.is_empty() {
                return Err(anyhow!("Library Skill display name cannot be empty"));
            }
            let description = description
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            db.update_library_skill_display_metadata(
                id,
                display_name,
                description.as_deref(),
                Utc::now().timestamp(),
            )?
            .ok_or_else(|| anyhow!("Library Skill not found: {id}"))
        })();
        Self::record_library_activity(
            db,
            ActivityReason::MetadataUpdate,
            if result.is_ok() {
                ActivityOutcome::Success
            } else {
                ActivityOutcome::Failed
            },
            result
                .as_ref()
                .err()
                .map(Self::activity_detail_for_error)
                .unwrap_or(ActivityDetailCode::None),
            Some(id.to_string()),
            None,
        );
        result
    }

    fn activity_detail_for_error(error: &anyhow::Error) -> ActivityDetailCode {
        for source in error.chain() {
            if let Some(error) = source.downcast_ref::<AppError>() {
                return match error {
                    AppError::Database(_) => ActivityDetailCode::DatabaseFailure,
                    AppError::InvalidInput(_) | AppError::Config(_) => {
                        ActivityDetailCode::InvalidInput
                    }
                    AppError::Io { .. } | AppError::IoContext { .. } => {
                        ActivityDetailCode::FilesystemFailure
                    }
                    _ => ActivityDetailCode::ValidationFailure,
                };
            }
            if source.downcast_ref::<std::io::Error>().is_some() {
                return ActivityDetailCode::FilesystemFailure;
            }
        }
        ActivityDetailCode::ValidationFailure
    }

    fn record_admission_result(db: &Arc<Database>, result: &Result<LibraryAdmission>) {
        match result {
            Ok(admission) => Self::record_library_activity(
                db,
                ActivityReason::Acquire,
                match admission.disposition {
                    LibraryAdmissionDisposition::Created => ActivityOutcome::Success,
                    LibraryAdmissionDisposition::Reused => ActivityOutcome::NoOp,
                },
                match admission.disposition {
                    LibraryAdmissionDisposition::Created => ActivityDetailCode::None,
                    LibraryAdmissionDisposition::Reused => ActivityDetailCode::AlreadyInSync,
                },
                Some(admission.skill.id.clone()),
                None,
            ),
            Err(error) => Self::record_library_activity(
                db,
                ActivityReason::Acquire,
                ActivityOutcome::Failed,
                Self::activity_detail_for_error(error),
                None,
                None,
            ),
        }
    }

    fn record_library_activity(
        db: &Arc<Database>,
        reason: ActivityReason,
        outcome: ActivityOutcome,
        detail_code: ActivityDetailCode,
        library_skill_id: Option<String>,
        batch: Option<ActivityBatchContext>,
    ) {
        ActivityRecorder::new(db.clone()).record_best_effort(ActivityEventInput {
            operation: ActivityOperation::Library,
            reason,
            outcome,
            actor: ActivityActor::User,
            trigger: if batch.is_some() {
                ActivityTrigger::Batch
            } else {
                ActivityTrigger::Command
            },
            target: ActivityTarget {
                library_skill_id,
                ..ActivityTarget::default()
            },
            batch,
            detail_code,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 构造一个模拟 GitHub 归档的 ZIP：带一层 `repo-main/` 根目录，
    /// 其中掺入用 `../` 逃逸的恶意条目。
    ///
    /// 两个恶意条目走的是**不同**的拦截层，缺一不可：
    /// - 两级 `../../`：净深度为负，`enclosed_name()` 自己就会拒绝；
    /// - 一级 `../`：净深度非负，`enclosed_name()` **放行**且原样保留 `..`，
    ///   只有剥掉 root_name 之后的组件校验才能拦住。
    fn build_zip_with_traversal_entry() -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();

            // 合法条目：会被正常解压
            zip.start_file("repo-main/SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: ok\n---\n").unwrap();

            // 恶意条目 A：被 enclosed_name() 拒绝
            zip.start_file("repo-main/../../escaped.txt", opts).unwrap();
            zip.write_all(b"pwned").unwrap();

            // 恶意条目 B：能通过 enclosed_name()，靠组件校验拦截
            zip.start_file("repo-main/../escaped-one-level.txt", opts)
                .unwrap();
            zip.write_all(b"pwned").unwrap();

            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn validate_repo_ref_accepts_real_world_coordinates() {
        // 合法分支名允许 `/`，不能因为防穿越就把它们一起禁掉
        for branch in [
            "main",
            "master",
            "HEAD",
            "feature/new-thing",
            "release/v1.2.3",
            "fix-123",
            "user.name/topic",
        ] {
            assert!(
                SkillService::validate_repo_ref("farion1231", "cc-switch", branch).is_ok(),
                "must accept branch: {branch:?}"
            );
        }
        assert!(SkillService::validate_repo_ref("a", "b.c_d-e", "main").is_ok());
    }

    #[test]
    fn validate_repo_ref_accepts_the_empty_branch_sentinel() {
        // 空 branch 与 "HEAD" 在 download_repo 里是同一个哨兵：分支候选表跳过
        // 两者，改试 main / master，所以它们从不进 URL。校验若把空串当非法，
        // 存量 skill_repos 行（建表默认 'main'，但空串没被禁）会在 download_repo
        // 第一行就 INVALID_REPO_REF，整个技能面板列不出东西——前端两处
        // `repo.branch || "main"` 正是照着"空串可用"写的。
        assert!(
            SkillService::validate_repo_ref("farion1231", "cc-switch", "").is_ok(),
            "the empty-branch sentinel must stay usable"
        );
    }

    #[test]
    fn validate_repo_ref_rejects_url_hijacking_branches() {
        // 这是核心用例：branch 被拼进 archive URL，URL 解析会消解点段，
        // 落点会从 /archive/refs/heads/ 改写成攻击者可上传的 release asset。
        for branch in [
            "../../../releases/download/v1/evil",
            "..",
            "../x",
            "a/../../b",
            "a/./b",
            "..\\..\\releases\\download\\v1\\evil",
            "/leading",
            "trailing/",
            "double//slash",
            "with space",
            "frag#ment",
            "pct%2e%2e",
            "ref@{0}",
            "seg.lock",
            ".hidden/x",
        ] {
            assert!(
                SkillService::validate_repo_ref("owner", "repo", branch).is_err(),
                "must reject branch: {branch:?}"
            );
        }
        for (owner, name) in [
            ("..", "repo"),
            ("own/er", "repo"),
            ("owner", ".."),
            ("owner", "re/po"),
            ("owner", "re po"),
            ("", "repo"),
            ("owner", ""),
        ] {
            assert!(
                SkillService::validate_repo_ref(owner, name, "main").is_err(),
                "must reject coordinates: {owner:?}/{name:?}"
            );
        }
    }

    #[test]
    fn assert_github_archive_url_pins_host_and_path() {
        let ok = "https://github.com/owner/repo/archive/refs/heads/main.zip";
        assert!(SkillService::assert_github_archive_url(ok, "owner", "repo").is_ok());

        // 出口断言必须挡住落点被改写到 release asset 的情况
        for bad in [
            "https://github.com/owner/repo/releases/download/v1/evil.zip",
            "https://evil.example/owner/repo/archive/refs/heads/main.zip",
            "http://github.com/owner/repo/archive/refs/heads/main.zip",
            "https://github.com/other/repo/archive/refs/heads/main.zip",
        ] {
            assert!(
                SkillService::assert_github_archive_url(bad, "owner", "repo").is_err(),
                "must reject url: {bad}"
            );
        }
    }

    #[test]
    fn build_skill_doc_url_drops_illegal_coordinates() {
        assert_eq!(
            SkillService::build_skill_doc_url("owner", "repo", "main", "a/SKILL.md").as_deref(),
            Some("https://github.com/owner/repo/blob/main/a/SKILL.md")
        );
        // readme_url 会被前端 openExternal 直接打开，非法坐标不得产出链接
        assert!(
            SkillService::build_skill_doc_url("owner", "repo", "../../../issues", "x").is_none()
        );
    }

    #[test]
    fn copy_entry_within_budget_stops_before_exceeding_the_limit() {
        // 预算逐块累加，超限时中止且不再继续写——压缩炸弹声明的 size 不可信，
        // 所以判断只能基于实际读到的字节。
        let mut total = MAX_ARCHIVE_TOTAL_BYTES - 8;
        let mut reader = std::io::Cursor::new(vec![7u8; 64]);
        let mut writer: Vec<u8> = Vec::new();

        let err = SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
            .expect_err("must reject once the budget is exhausted");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
        assert!(
            writer.is_empty(),
            "nothing may be written once the chunk would exceed the budget"
        );

        // 预算充足时照常写完
        let mut total = 0u64;
        let mut reader = std::io::Cursor::new(vec![7u8; 64]);
        let mut writer: Vec<u8> = Vec::new();
        SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
            .expect("within budget");
        assert_eq!(writer.len(), 64);
        assert_eq!(total, 64);
    }

    #[test]
    fn extract_repo_archive_rejects_too_many_entries() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            for i in 0..(MAX_ARCHIVE_ENTRIES + 1) {
                zip.start_file(format!("repo-main/f{i}"), opts).unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");
        let err = SkillService::extract_repo_archive(archive, temp.path())
            .expect_err("entry count over the limit must be rejected");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_MANY_ENTRIES"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn extract_repo_archive_rejects_path_traversal_entries() {
        let temp = tempdir().expect("tempdir");
        // dest 放在深一层，这样逃逸一层/两层都落在 temp 内、可被检出
        let dest = temp.path().join("nested").join("dest");
        fs::create_dir_all(&dest).expect("create dest");

        let bytes = build_zip_with_traversal_entry();
        let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("archive parses");

        SkillService::extract_repo_archive(archive, &dest).expect("extract must not fail");

        // 合法条目正常落盘
        assert!(
            dest.join("SKILL.md").is_file(),
            "legitimate entry should be extracted"
        );
        // 两级逃逸：不得写到 dest 之外
        assert!(
            !temp.path().join("escaped.txt").exists(),
            "zip-slip entry must not escape dest (temp root)"
        );
        assert!(
            !temp.path().join("nested").join("escaped.txt").exists(),
            "zip-slip entry must not escape dest (parent dir)"
        );
        // 一级逃逸：enclosed_name() 放行的那一类，必须被组件校验拦住
        assert!(
            !temp
                .path()
                .join("nested")
                .join("escaped-one-level.txt")
                .exists(),
            "single-`..` entry must not escape dest (enclosed_name allows it)"
        );
    }

    #[test]
    fn extract_repo_archive_skips_a_symlink_that_contains_itself() {
        // `dir/link -> ..` 解析后正是归档根：它**通过**「目标必须在 base 内」的
        // 检查，因为目标就是 base 本身。没有第二道自包含检查时，
        // copy_dir_recursive(base, base/dir/link) 会把根复制进自己的子目录，
        // 每递归一层都重新看到刚落盘的副本，直到 PATH_MAX 才以 IO 错误收场。
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            zip.start_file("repo-main/SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: t\ndescription: d\n---\n")
                .unwrap();
            zip.add_directory("repo-main/dir/", opts).unwrap();
            zip.add_symlink("repo-main/dir/link", "..", opts).unwrap();
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let dest = temp.path().join("dest");
        fs::create_dir_all(&dest).expect("create dest");
        let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");

        SkillService::extract_repo_archive(archive, &dest)
            .expect("a self-containing symlink must be skipped, not blow up the extraction");

        assert!(
            dest.join("SKILL.md").is_file(),
            "legitimate entries must still be extracted"
        );
        assert!(
            !dest.join("dir").join("link").exists(),
            "a symlink whose target contains the link itself must not be materialized"
        );
    }

    #[test]
    fn symlink_materialization_is_charged_to_the_archive_budget() {
        // symlink 的物化走第二遍、与解压循环不同的代码路径。若它不计入同一个
        // 预算，「一个大文件 + N 个指向它的 symlink」就能写下 N 倍字节而上限
        // 全程显示合规。这里把预算预置到接近上限来验证物化确实在计费。
        let temp = tempdir().expect("tempdir");
        let base = temp.path().join("base");
        fs::create_dir_all(base.join("payload")).expect("create payload dir");
        fs::write(base.join("payload").join("big.bin"), vec![b'x'; 4096]).expect("write payload");

        let symlinks = vec![(base.join("copy"), "payload".to_string())];
        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 1024;

        let err = SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
            .expect_err("materializing 4 KiB with 1 KiB of budget left must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn symlink_guard_sees_through_unnormalized_link_paths() {
        // `enclosed_name()` 不消解 `..`，所以 link_path 可能长成 `e/../d/self`。
        // 守卫若拿它按字面跟规范化过的目标比组件，会在第一段（`e` vs `d`）就判定
        // "不包含"——而这个位置物理上就在 `d` 里面，把 `d` 复制进去正是递归自复制。
        let temp = tempdir().expect("tempdir");
        let base = temp.path().join("base");
        fs::create_dir_all(base.join("d").join("sub")).expect("create d");
        fs::create_dir_all(base.join("e")).expect("create e");

        let link_path = base.join("e").join("..").join("d").join("self");
        let symlinks = vec![(link_path, ".".to_string())];
        let mut total_bytes = 0u64;

        SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
            .expect("a self-containing symlink must be skipped, not blow up the extraction");

        assert!(
            !base.join("d").join("self").exists(),
            "a link that physically lives inside its own target must not be materialized"
        );
    }

    /// 记录实际被消耗了多少字节的 reader。
    ///
    /// 直接断言返回值是不够的：函数末尾本就有一道长度检查，把 `take` 的上限拆掉
    /// 之后它照样返回 `None`，断言仍然通过——而炸弹的危害全在读取过程里，不在
    /// 返回值。只有观测消耗量才能真正钉住"读取是有界的"。
    struct CountingReader {
        remaining: u64,
        consumed: u64,
    }

    impl std::io::Read for CountingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = buf.len().min(self.remaining as usize);
            buf[..n].fill(b'a');
            self.remaining -= n as u64;
            self.consumed += n as u64;
            Ok(n)
        }
    }

    #[test]
    fn read_symlink_target_is_bounded_and_charged() {
        // 一个打着 symlink 标志、解压流却极大的条目：zip 2.4.2 的 make_reader
        // 不按声明的 uncompressed_size 截断，没有上限就会被整条读进内存。
        let mut oversized = CountingReader {
            remaining: 8 * 1024 * 1024,
            consumed: 0,
        };
        let mut total_bytes = 0u64;

        let target = SkillService::read_symlink_target(&mut oversized, &mut total_bytes)
            .expect("an oversized target must be skipped, not raise");
        assert!(
            target.is_none(),
            "a target longer than a path can plausibly be must be rejected"
        );
        assert_eq!(total_bytes, 0, "a rejected target must not be charged");
        assert!(
            oversized.consumed <= MAX_SYMLINK_TARGET_BYTES + 1,
            "the read must stop at the cap instead of draining the stream, consumed {}",
            oversized.consumed
        );

        // 正常目标照常读出来并计费
        let mut normal = std::io::Cursor::new(b"../shared".to_vec());
        let target = SkillService::read_symlink_target(&mut normal, &mut total_bytes)
            .expect("a normal target must be read");
        assert_eq!(target.as_deref(), Some("../shared"));
        assert_eq!(total_bytes, 9);
    }

    #[test]
    fn directory_materialization_is_charged_to_the_archive_budget() {
        // 全是空目录的归档一个内容字节都不写。不给目录计费，第二遍的 symlink
        // 解析就能让目录数按层数指数增长，而预算读数一直停在 0。
        let temp = tempdir().expect("tempdir");
        let src = temp.path().join("src");
        fs::create_dir_all(src.join("a").join("b")).expect("create tree");

        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - DIRECTORY_BUDGET_COST;
        let err =
            SkillService::copy_dir_within_budget(&src, &temp.path().join("dest"), &mut total_bytes)
                .expect_err("materializing directories past the limit must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn create_dir_all_charges_every_directory_it_creates() {
        // `create_dir_all` 一次能把缺失的父目录全建出来，所以一个条目名
        // `a/a/…/a/f.txt` 可以隐式造出几百层。按调用次数计费会严重低估。
        let temp = tempdir().expect("tempdir");
        let deep = temp.path().join("a").join("b").join("c");

        // 预算只够两层，建三层必须被拦下
        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 2 * DIRECTORY_BUDGET_COST;
        let err = SkillService::create_dir_all_within_budget(&deep, &mut total_bytes)
            .expect_err("creating more directories than the budget allows must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
        assert!(
            !deep.exists(),
            "nothing must be created once the budget is exceeded"
        );
    }

    fn write_skill(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("create skill dir");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\n"),
        )
        .expect("write SKILL.md");
    }

    #[test]
    // serial：与 backup/s3_sync/deeplink 等同样读写进程级 CC_SWITCH_TEST_HOME 的测试互斥，
    // EnvGuard 只负责恢复不提供互斥。
    #[serial_test::serial]
    fn get_app_skills_dir_honors_test_home_override() {
        // 回归：曾直呼 dirs::home_dir() 绕过 CC_SWITCH_TEST_HOME——Unix 上碰巧跟 $HOME
        // 一致所以测试能过，Windows 上 dirs 走 Known Folder API，测试隔离整体失效
        // （tests/skill_sync.rs 扫到 runner 真实用户目录）。
        struct EnvGuard(Option<std::ffi::OsString>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                    None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
                }
            }
        }
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard(std::env::var_os("CC_SWITCH_TEST_HOME"));
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let dir =
            SkillService::get_app_skills_dir(&AppType::Claude).expect("resolve claude skills dir");
        assert!(
            dir.starts_with(temp.path()),
            "skills dir must live under the overridden test home, got {}",
            dir.display()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pi_is_not_exposed_as_a_redesigned_skill_consumer() {
        let error = SkillService::get_app_skills_dir(&AppType::Pi)
            .expect_err("Pi must stay outside the redesigned Skill consumer boundary");
        assert!(
            error
                .to_string()
                .contains("not yet a redesigned Skill consumer"),
            "unexpected boundary error: {error}"
        );
    }

    #[test]
    fn resolve_skill_source_dir_returns_repo_root_for_root_level_skill() {
        let temp = tempdir().expect("tempdir");
        write_skill(temp.path(), "Root Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "last30days-skill-cn")
            .expect("root-level skill should resolve to the extracted repo root");

        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn resolve_skill_source_dir_returns_direct_nested_directory_when_present() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("skills").join("nested-skill");
        write_skill(&nested, "Nested Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "skills/nested-skill")
            .expect("nested skill should resolve from its relative source path");

        assert_eq!(resolved, nested);
    }

    #[test]
    fn resolve_skill_source_dir_falls_back_to_matching_install_name() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("skills").join("nested-skill");
        write_skill(&nested, "Nested Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "nested-skill")
            .expect("install name should fall back to the matching discovered skill directory");

        assert_eq!(resolved, nested);
    }

    #[test]
    fn resolve_skill_source_dir_rejects_same_name_wrapper_without_skill_md() {
        // 复刻 issue #4141：ast-grep/agent-skill 结构。仓库根下有同名目录 ast-grep/
        // （plugin 包，无 SKILL.md），真正的 skill 在 ast-grep/skills/ast-grep/SKILL.md。
        let temp = tempdir().expect("tempdir");
        let wrapper = temp.path().join("ast-grep");
        fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
        fs::write(
            wrapper.join(".claude-plugin").join("plugin.json"),
            "{\"name\":\"ast-grep\"}",
        )
        .expect("write plugin.json");
        let real_skill = wrapper.join("skills").join("ast-grep");
        write_skill(&real_skill, "ast-grep");

        // directory 只给了 skill 名 "ast-grep"（skills.sh API 的语义），不能命中空壳 wrapper。
        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep")
            .expect("should resolve to the inner skill dir, not the same-name wrapper");

        assert_eq!(resolved, real_skill);
        assert!(resolved.join("SKILL.md").is_file());
    }

    #[test]
    fn resolve_skill_source_dir_finds_two_level_catalog_skill() {
        // catalog layout：skills/category/foo/SKILL.md（depth 3，find_skill_dir_by_name 可达）。
        let temp = tempdir().expect("tempdir");
        let catalog_skill = temp.path().join("skills").join("category").join("foo");
        write_skill(&catalog_skill, "Foo Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo")
            .expect("should resolve the two-level catalog skill by name");

        assert_eq!(resolved, catalog_skill);
    }

    #[test]
    fn resolve_skill_source_dir_returns_none_for_wrapper_without_inner_skill() {
        // 同名 wrapper 存在、无 SKILL.md，且无 inner skill / root SKILL.md 可兜底时，
        // 必须返回 None——守住 #4141 这个 bug class 的负例（不能把空壳目录当源目录）。
        let temp = tempdir().expect("tempdir");
        let wrapper = temp.path().join("ast-grep");
        fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
        fs::write(
            wrapper.join(".claude-plugin").join("plugin.json"),
            "{\"name\":\"ast-grep\"}",
        )
        .expect("write plugin.json");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep");
        assert!(
            resolved.is_none(),
            "wrapper dir without SKILL.md and no inner skill must resolve to None, got {:?}",
            resolved
        );
    }

    #[test]
    fn resolve_skill_source_dir_returns_none_when_no_skill_md_anywhere() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills").join("foo")).expect("create empty skill dir");
        fs::write(temp.path().join("README.md"), "no skills here").expect("write README");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo");
        assert!(
            resolved.is_none(),
            "no SKILL.md anywhere must resolve to None"
        );
    }

    #[test]
    fn doc_path_for_source_returns_repo_relative_skill_md_path() {
        let temp = tempdir().expect("tempdir");
        let nested = temp
            .path()
            .join("skills")
            .join("developertools")
            .join("solutions")
            .join("foo");
        fs::create_dir_all(&nested).expect("create nested dirs");

        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), &nested),
            Some("skills/developertools/solutions/foo/SKILL.md".to_string())
        );
        // 源目录即仓库根：文档路径就是根下的 SKILL.md
        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), temp.path()),
            Some("SKILL.md".to_string())
        );
        // 仓库根之外：None（调用方已做包含性校验，防御性兜底）
        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), std::path::Path::new("/elsewhere")),
            None
        );
    }
}

#[cfg(test)]
mod library_hash_regressions {
    use super::LibrarySkillAcquisitionService as Service;
    use std::fs;

    #[test]
    fn framed_hash_distinguishes_file_boundaries_and_preserves_legacy_baselines() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fs::write(a.path().join("a"), b"\0b\0file\0x").unwrap();
        fs::write(b.path().join("a"), b"").unwrap();
        fs::write(b.path().join("b"), b"x").unwrap();
        let legacy = Service::compute_legacy_library_hash(a.path()).unwrap();
        assert_eq!(
            legacy,
            Service::compute_legacy_library_hash(b.path()).unwrap()
        );
        assert_ne!(
            Service::compute_library_hash(a.path()).unwrap(),
            Service::compute_library_hash(b.path()).unwrap()
        );
        assert!(Service::hash_matches_baseline(a.path(), &legacy).unwrap());
        fs::write(a.path().join("a"), b"changed").unwrap();
        assert!(!Service::hash_matches_baseline(a.path(), &legacy).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn executable_change_is_content_change() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("run.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
        let baseline = Service::compute_library_hash(root.path()).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Service::hash_matches_baseline(root.path(), &baseline).unwrap());
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!Service::hash_matches_baseline(root.path(), &baseline).unwrap());
    }
}
