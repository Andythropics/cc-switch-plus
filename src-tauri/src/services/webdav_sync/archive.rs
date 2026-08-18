use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::DateTime;

use crate::error::AppError;
#[cfg(target_os = "macos")]
use crate::services::skill::LibrarySkillAcquisitionService;

use crate::services::sync_protocol::{
    io_context_localized, localized, MAX_SYNC_ARTIFACT_BYTES, REMOTE_SKILLS_ZIP,
};

/// Maximum number of entries allowed in a zip archive.
const MAX_EXTRACT_ENTRIES: usize = 10_000;
const NO_PORTABLE_SKILLS_MARKER: &str = "__cc_switch_no_portable_skills__";

#[cfg(target_os = "macos")]
fn portable_skills_root() -> PathBuf {
    LibrarySkillAcquisitionService::library_directory_path()
}

#[cfg(target_os = "macos")]
pub(crate) fn zip_skills_ssot(dest_path: &Path) -> Result<(), AppError> {
    let source = portable_skills_root();
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let file = fs::File::create(dest_path).map_err(|e| AppError::io(dest_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    if source.exists() {
        let canonical_root = fs::canonicalize(&source).unwrap_or_else(|_| source.clone());
        let mut visited = HashSet::new();
        mark_visited_dir(&canonical_root, &mut visited)?;
        zip_dir_recursive(
            &canonical_root,
            &canonical_root,
            &mut writer,
            options,
            &mut visited,
        )?;
    }

    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.skills_zip_write_failed",
            format!("写入 skills.zip 失败: {e}"),
            format!("Failed to write skills.zip: {e}"),
        )
    })?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn zip_skills_ssot(dest_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let file = fs::File::create(dest_path).map_err(|e| AppError::io(dest_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(
            NO_PORTABLE_SKILLS_MARKER,
            SimpleFileOptions::default().last_modified_time(DateTime::default()),
        )
        .map_err(|e| {
            localized(
                "webdav.sync.zip_start_file_failed",
                format!("写入 Skills 平台标记失败: {e}"),
                format!("Failed to write Skills platform marker: {e}"),
            )
        })?;
    writer.write_all(b"preserve-device-skills").map_err(|e| {
        localized(
            "webdav.sync.zip_write_file_failed",
            format!("写入 Skills 平台标记失败: {e}"),
            format!("Failed to write Skills platform marker: {e}"),
        )
    })?;
    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.skills_zip_write_failed",
            format!("写入 skills.zip 失败: {e}"),
            format!("Failed to write skills.zip: {e}"),
        )
    })?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn restore_skills_zip(raw: &[u8]) -> Result<(), AppError> {
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.skills_extract_tmpdir_failed",
            "创建 skills 解压临时目录失败",
            "Failed to create temporary directory for skills extraction",
            e,
        )
    })?;
    let zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    fs::write(&zip_path, raw).map_err(|e| AppError::io(&zip_path, e))?;

    let file = fs::File::open(&zip_path).map_err(|e| AppError::io(&zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        localized(
            "webdav.sync.skills_zip_parse_failed",
            format!("解析 skills.zip 失败: {e}"),
            format!("Failed to parse skills.zip: {e}"),
        )
    })?;
    if archive.len() == 1
        && archive
            .by_index(0)
            .map(|entry| entry.name() == NO_PORTABLE_SKILLS_MARKER)
            .unwrap_or(false)
    {
        return Ok(());
    }

    let extracted = tmp.path().join("skills-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;

    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(localized(
            "webdav.sync.skills_zip_too_many_entries",
            format!(
                "skills.zip 条目数过多（{}），上限 {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
            format!(
                "skills.zip has too many entries ({}), limit is {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
        ));
    }

    let mut total_bytes: u64 = 0;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            localized(
                "webdav.sync.skills_zip_entry_read_failed",
                format!("读取 ZIP 项失败: {e}"),
                format!("Failed to read ZIP entry: {e}"),
            )
        })?;
        let Some(safe_name) = entry.enclosed_name() else {
            continue;
        };
        let out_path = extracted.join(&safe_name);
        let unix_mode = entry.unix_mode();
        ensure_no_symlink_ancestors(&extracted, &out_path)?;
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| AppError::io(&out_path, e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        if entry.is_symlink() {
            let mut target = Vec::new();
            copy_entry_with_total_limit(
                &mut entry,
                &mut target,
                &mut total_bytes,
                MAX_SYNC_ARTIFACT_BYTES,
                &out_path,
            )?;
            let target = String::from_utf8(target).map_err(|error| {
                localized(
                    "webdav.sync.skills_zip_entry_read_failed",
                    format!("符号链接目标不是有效 UTF-8: {error}"),
                    format!("Symlink target is not valid UTF-8: {error}"),
                )
            })?;
            let target = Path::new(&target);
            if !relative_symlink_is_contained(&safe_name, target) {
                return Err(localized(
                    "webdav.sync.skills_zip_entry_read_failed",
                    "符号链接目标超出 Skills 目录".to_string(),
                    "Symlink target escapes the Skills directory".to_string(),
                ));
            }
            create_symlink(target, &out_path)?;
            continue;
        }

        let mut out = fs::File::create(&out_path).map_err(|e| AppError::io(&out_path, e))?;
        let _written = copy_entry_with_total_limit(
            &mut entry,
            &mut out,
            &mut total_bytes,
            MAX_SYNC_ARTIFACT_BYTES,
            &out_path,
        )?;
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o7777))
                .map_err(|e| AppError::io(&out_path, e))?;
        }
    }

    validate_extracted_library(&extracted)?;

    let ssot = portable_skills_root();
    let parent = ssot.parent().ok_or_else(|| {
        localized(
            "webdav.sync.skills_extract_tmpdir_failed",
            "Library 路径缺少父目录".to_string(),
            "Library path has no parent directory".to_string(),
        )
    })?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    let stage = parent.join(format!(
        ".skills-sync-stage-{}",
        uuid::Uuid::new_v4().simple()
    ));
    copy_dir_recursive(&extracted, &stage)?;
    sync_tree(&stage)?;

    if ssot.exists() {
        atomic_swap_dirs(&ssot, &stage).map_err(|e| AppError::io(&ssot, e))?;
        sync_directory(parent)?;
        fs::remove_dir_all(&stage).map_err(|e| AppError::io(&stage, e))?;
    } else {
        fs::rename(&stage, &ssot).map_err(|e| AppError::io(&ssot, e))?;
        sync_directory(parent)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restore_skills_zip(_raw: &[u8]) -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn backup_current_skills(backup_dir: &Path) -> Result<bool, AppError> {
    let ssot = portable_skills_root();
    let existed = ssot.exists();
    if existed {
        copy_dir_recursive(&ssot, backup_dir)?;
        sync_tree(backup_dir)?;
    }
    Ok(existed)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn backup_current_skills(_backup_dir: &Path) -> Result<bool, AppError> {
    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn restore_skills_from_backup(backup_dir: &Path, existed: bool) -> Result<(), AppError> {
    let ssot = portable_skills_root();
    let parent = ssot.parent().ok_or_else(|| {
        localized(
            "webdav.sync.skills_backup_tmpdir_failed",
            "Library 路径缺少父目录".to_string(),
            "Library path has no parent directory".to_string(),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    let stage = parent.join(format!(
        ".skills-sync-recovery-stage-{}",
        uuid::Uuid::new_v4().simple()
    ));

    if existed {
        if !backup_dir.is_dir() {
            return Err(localized(
                "webdav.sync.skills_backup_tmpdir_failed",
                "Skills 恢复备份不存在".to_string(),
                "Skills recovery backup is missing".to_string(),
            ));
        }
        copy_dir_recursive(backup_dir, &stage)?;
        sync_tree(&stage)?;
        if ssot.exists() {
            atomic_swap_dirs(&ssot, &stage).map_err(|error| AppError::io(&ssot, error))?;
            sync_directory(parent)?;
            fs::remove_dir_all(&stage).map_err(|error| AppError::io(&stage, error))?;
        } else {
            fs::rename(&stage, &ssot).map_err(|error| AppError::io(&ssot, error))?;
            sync_directory(parent)?;
        }
    } else if ssot.exists() {
        fs::rename(&ssot, &stage).map_err(|error| AppError::io(&ssot, error))?;
        sync_directory(parent)?;
        fs::remove_dir_all(&stage).map_err(|error| AppError::io(&stage, error))?;
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restore_skills_from_backup(
    _backup_dir: &Path,
    _existed: bool,
) -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn sync_tree(root: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| AppError::io(root, error))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return fs::File::open(root)
            .and_then(|file| file.sync_all())
            .map_err(|error| AppError::io(root, error));
    }

    for entry in fs::read_dir(root).map_err(|error| AppError::io(root, error))? {
        let entry = entry.map_err(|error| AppError::io(root, error))?;
        sync_tree(&entry.path())?;
    }
    sync_directory(root)
}

#[cfg(target_os = "macos")]
fn sync_directory(path: &Path) -> Result<(), AppError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| AppError::io(path, error))
}

#[cfg(target_os = "macos")]
fn atomic_swap_dirs(left: &Path, right: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let left = CString::new(left.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
    let right = CString::new(right.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
    let result = unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn zip_dir_recursive(
    root: &Path,
    current: &Path,
    writer: &mut zip::ZipWriter<fs::File>,
    options: SimpleFileOptions,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut entries: Vec<_> = fs::read_dir(current)
        .map_err(|e| AppError::io(current, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::io(current, e))?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let rel = path.strip_prefix(root).map_err(|e| {
            localized(
                "webdav.sync.zip_relative_path_failed",
                format!("生成 ZIP 相对路径失败: {e}"),
                format!("Failed to build relative ZIP path: {e}"),
            )
        })?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let metadata = fs::symlink_metadata(&path).map_err(|e| AppError::io(&path, e))?;

        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|e| AppError::io(&path, e))?;
            let canonical_target = fs::canonicalize(&path).map_err(|e| AppError::io(&path, e))?;
            let skill_root = rel
                .components()
                .next()
                .and_then(|component| match component {
                    Component::Normal(name) => Some(root.join(name)),
                    _ => None,
                })
                .ok_or_else(|| {
                    localized(
                        "webdav.sync.skills_zip_entry_read_failed",
                        format!("无效的 Library 符号链接路径: {}", path.display()),
                        format!("Invalid Library symlink path: {}", path.display()),
                    )
                })?;
            let canonical_skill_root =
                fs::canonicalize(&skill_root).map_err(|e| AppError::io(&skill_root, e))?;
            if target.is_absolute() || !canonical_target.starts_with(&canonical_skill_root) {
                return Err(localized(
                    "webdav.sync.skills_zip_entry_read_failed",
                    format!("符号链接超出所属 Skill: {}", path.display()),
                    format!("Symlink escapes its Library Skill: {}", path.display()),
                ));
            }
            writer
                .add_symlink_from_path(rel, &target, options)
                .map_err(|e| {
                    localized(
                        "webdav.sync.zip_start_file_failed",
                        format!("写入 ZIP 符号链接失败: {e}"),
                        format!("Failed to write ZIP symlink entry: {e}"),
                    )
                })?;
            continue;
        }

        #[cfg(unix)]
        let entry_options = options.unix_permissions(metadata.permissions().mode() & 0o7777);
        #[cfg(not(unix))]
        let entry_options = options;

        if metadata.is_dir() {
            if !mark_visited_dir(&path, visited)? {
                log::warn!(
                    "[WebDAV] Skipping already visited directory: {}",
                    path.display()
                );
                continue;
            }
            writer
                .add_directory(format!("{rel_str}/"), entry_options)
                .map_err(|e| {
                    localized(
                        "webdav.sync.zip_add_directory_failed",
                        format!("写入 ZIP 目录失败: {e}"),
                        format!("Failed to write ZIP directory entry: {e}"),
                    )
                })?;
            zip_dir_recursive(root, &path, writer, options, visited)?;
        } else {
            writer.start_file(&rel_str, entry_options).map_err(|e| {
                localized(
                    "webdav.sync.zip_start_file_failed",
                    format!("写入 ZIP 文件头失败: {e}"),
                    format!("Failed to start ZIP file entry: {e}"),
                )
            })?;
            let mut file = fs::File::open(&path).map_err(|e| AppError::io(&path, e))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| AppError::io(&path, e))?;
            writer.write_all(&buf).map_err(|e| {
                localized(
                    "webdav.sync.zip_write_file_failed",
                    format!("写入 ZIP 文件内容失败: {e}"),
                    format!("Failed to write ZIP file content: {e}"),
                )
            })?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    let mut visited = HashSet::new();
    copy_dir_recursive_inner(src, dest, &mut visited)
}

fn copy_dir_recursive_inner(
    src: &Path,
    dest: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let metadata = match fs::symlink_metadata(src) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::io(src, error)),
    };
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(src).map_err(|e| AppError::io(src, e))?;
        create_symlink(&target, dest)?;
        return Ok(());
    }
    if !mark_visited_dir(src, visited)? {
        log::warn!(
            "[WebDAV] Skipping already visited copy path: {}",
            src.display()
        );
        return Ok(());
    }
    fs::create_dir_all(dest).map_err(|e| AppError::io(dest, e))?;
    for entry in fs::read_dir(src).map_err(|e| AppError::io(src, e))? {
        let entry = entry.map_err(|e| AppError::io(src, e))?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = fs::symlink_metadata(&path).map_err(|e| AppError::io(&path, e))?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|e| AppError::io(&path, e))?;
            create_symlink(&target, &dest_path)?;
        } else if metadata.is_dir() {
            copy_dir_recursive_inner(&path, &dest_path, visited)?;
        } else {
            fs::copy(&path, &dest_path).map_err(|e| AppError::io(&dest_path, e))?;
        }
    }
    Ok(())
}

fn relative_symlink_is_contained(link_path: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }

    let Some(Component::Normal(skill_directory)) = link_path.components().next() else {
        return false;
    };
    let mut resolved = link_path
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| match component {
                    Component::Normal(name) => Some(name.to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => resolved.push(name.to_owned()),
            Component::ParentDir if !resolved.is_empty() => {
                resolved.pop();
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved
        .first()
        .is_some_and(|component| component == skill_directory)
}

#[cfg(target_os = "macos")]
fn validate_extracted_library(root: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(root).map_err(|e| AppError::io(root, e))? {
        let entry = entry.map_err(|e| AppError::io(root, e))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| AppError::io(&path, e))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(localized(
                "webdav.sync.skills_zip_entry_read_failed",
                format!("Library 根目录只能包含 Skill 目录: {}", path.display()),
                format!(
                    "Library root may contain only Skill directories: {}",
                    path.display()
                ),
            ));
        }
        LibrarySkillAcquisitionService::validate_internal_symlinks(&path).map_err(|error| {
            localized(
                "webdav.sync.skills_zip_entry_read_failed",
                format!("Library Skill 校验失败: {error}"),
                format!("Library Skill validation failed: {error}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn validate_extracted_library(_root: &Path) -> Result<(), AppError> {
    Ok(())
}

fn ensure_no_symlink_ancestors(root: &Path, path: &Path) -> Result<(), AppError> {
    let relative = path.strip_prefix(root).map_err(|error| {
        localized(
            "webdav.sync.skills_zip_entry_read_failed",
            format!("ZIP 项超出 Skills 目录: {error}"),
            format!("ZIP entry escapes the Skills directory: {error}"),
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative
        .components()
        .take(relative.components().count().saturating_sub(1))
    {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(localized(
                    "webdav.sync.skills_zip_entry_read_failed",
                    format!("ZIP 项穿过符号链接: {}", current.display()),
                    format!("ZIP entry traverses a symlink: {}", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(AppError::io(&current, error)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    std::os::unix::fs::symlink(target, link).map_err(|e| AppError::io(link, e))
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    let target_is_dir = link
        .parent()
        .map(|parent| parent.join(target).is_dir())
        .unwrap_or(false);
    if target_is_dir {
        std::os::windows::fs::symlink_dir(target, link).map_err(|e| AppError::io(link, e))
    } else {
        std::os::windows::fs::symlink_file(target, link).map_err(|e| AppError::io(link, e))
    }
}

fn mark_visited_dir(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<bool, AppError> {
    let canonical = fs::canonicalize(path).map_err(|e| AppError::io(path, e))?;
    Ok(visited.insert(canonical))
}

fn copy_entry_with_total_limit<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    total_bytes: &mut u64,
    max_total_bytes: u64,
    out_path: &Path,
) -> Result<u64, AppError> {
    let mut buffer = [0u8; 16 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| AppError::io(out_path, e))?;
        if n == 0 {
            break;
        }

        if total_bytes.saturating_add(n as u64) > max_total_bytes {
            let max_mb = max_total_bytes / 1024 / 1024;
            return Err(localized(
                "webdav.sync.skills_zip_too_large",
                format!("skills.zip 解压后体积超过上限（{max_mb} MB）"),
                format!("skills.zip extracted size exceeds limit ({max_mb} MB)"),
            ));
        }

        writer
            .write_all(&buffer[..n])
            .map_err(|e| AppError::io(out_path, e))?;
        *total_bytes += n as u64;
        written += n as u64;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::{copy_entry_with_total_limit, mark_visited_dir};
    #[cfg(target_os = "macos")]
    use super::{restore_skills_zip, zip_skills_ssot};
    use std::collections::HashSet;
    use std::io::Cursor;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn mark_visited_dir_tracks_canonical_duplicates() {
        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join("skills");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut visited = HashSet::new();
        assert!(mark_visited_dir(&dir, &mut visited).expect("first visit"));
        assert!(!mark_visited_dir(&dir, &mut visited).expect("second visit"));
    }

    #[test]
    fn copy_entry_with_total_limit_rejects_oversized_stream_before_write() {
        let mut reader = Cursor::new(vec![1u8; 16]);
        let mut writer = Vec::new();
        let mut total_bytes = 0u64;

        let err = copy_entry_with_total_limit(
            &mut reader,
            &mut writer,
            &mut total_bytes,
            8,
            Path::new("skills-extracted/file.bin"),
        )
        .expect_err("stream larger than limit should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
        assert_eq!(
            writer.len(),
            0,
            "should not write when the first chunk exceeds limit"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[serial_test::serial]
    fn portable_archive_reads_private_library_not_legacy_consumer_storage() {
        let temp = tempdir().expect("tempdir");
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let library = temp.path().join(".cc-switch/skills/library-skill");
        let legacy = temp.path().join(".agents/skills/legacy-skill");
        std::fs::create_dir_all(&library).expect("create Library skill");
        std::fs::create_dir_all(&legacy).expect("create legacy skill");
        std::fs::write(library.join("SKILL.md"), "library").expect("write Library skill");
        std::fs::write(library.join(".portable-metadata"), "metadata")
            .expect("write hidden Library metadata");
        std::fs::write(library.join("shared.txt"), "shared").expect("write symlink target");
        let executable = library.join("run.sh");
        std::fs::write(&executable, "#!/bin/sh\necho portable\n").expect("write executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("mark executable");
        std::os::unix::fs::symlink("shared.txt", library.join("shared-link"))
            .expect("create contained relative symlink");
        std::fs::write(legacy.join("SKILL.md"), "legacy").expect("write legacy skill");

        let archive_path = temp.path().join("skills.zip");
        zip_skills_ssot(&archive_path).expect("archive portable Library");
        let archive_file = std::fs::File::open(&archive_path).expect("open archive");
        let mut archive = zip::ZipArchive::new(archive_file).expect("parse archive");
        let names = (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .expect("read archive entry")
                    .name()
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name == "library-skill/SKILL.md"));
        assert!(names
            .iter()
            .any(|name| name == "library-skill/.portable-metadata"));
        assert!(!names.iter().any(|name| name.contains("legacy-skill")));

        let raw = std::fs::read(&archive_path).expect("read archive");
        std::fs::remove_dir_all(temp.path().join(".cc-switch/skills"))
            .expect("remove Library before restore");
        restore_skills_zip(&raw).expect("restore portable Library");
        assert_eq!(
            std::fs::read_to_string(library.join(".portable-metadata"))
                .expect("read restored metadata"),
            "metadata"
        );
        assert_eq!(
            std::fs::read_link(library.join("shared-link")).expect("read restored symlink"),
            Path::new("shared.txt")
        );
        assert_eq!(
            std::fs::metadata(library.join("run.sh"))
                .expect("stat restored executable")
                .permissions()
                .mode()
                & 0o777,
            0o755,
            "portable archive must retain executable permission bits"
        );

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[serial_test::serial]
    fn restore_rejects_cross_skill_and_broken_links_before_replacing_library() {
        let temp = tempdir().expect("tempdir");
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let library = temp.path().join(".cc-switch/skills");
        std::fs::create_dir_all(library.join("existing")).expect("create existing Library skill");
        std::fs::write(library.join("existing/SKILL.md"), "unchanged")
            .expect("write existing Library skill");

        for (archive_name, target) in [
            ("cross-skill.zip", "../skill-b/target.txt"),
            ("broken-link.zip", "missing.txt"),
        ] {
            let archive_path = temp.path().join(archive_name);
            let file = std::fs::File::create(&archive_path).expect("create malicious archive");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer
                .add_directory("skill-a/", options)
                .expect("add skill-a");
            writer
                .start_file("skill-a/SKILL.md", options)
                .expect("add manifest");
            std::io::Write::write_all(&mut writer, b"skill-a").expect("write manifest");
            writer
                .add_directory("skill-b/", options)
                .expect("add skill-b");
            writer
                .start_file("skill-b/target.txt", options)
                .expect("add target");
            std::io::Write::write_all(&mut writer, b"target").expect("write target");
            writer
                .add_symlink("skill-a/link", target, options)
                .expect("add unsafe symlink");
            writer.finish().expect("finish archive");

            let raw = std::fs::read(&archive_path).expect("read malicious archive");
            restore_skills_zip(&raw).expect_err("unsafe links must fail closed");
            assert_eq!(
                std::fs::read_to_string(library.join("existing/SKILL.md"))
                    .expect("existing Library remains"),
                "unchanged"
            );
        }

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}
