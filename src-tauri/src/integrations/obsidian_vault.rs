//! Explicit, read-only access to one user-selected Obsidian vault.
//!
//! All traversal is relative to an opened directory descriptor. `O_NOFOLLOW`
//! applies to every component, so replacing a subdirectory with a symlink
//! between enumeration and reading cannot extend the grant to another folder.

use super::obsidian::ObsidianNote;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};

pub const MAX_NOTE_BYTES: usize = 1_048_576;
const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_SCAN_BYTES: usize = 16 * 1_048_576;
const MAX_DEPTH: usize = 32;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_SNIPPET_CHARS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultGrant {
    pub root: String,
    device: u64,
    inode: u64,
}

impl VaultGrant {
    pub fn name(&self) -> String {
        Path::new(&self.root)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Obsidian".to_string())
    }

    /// Only called with the native folder picker's selected path.
    pub fn authorize(path: &Path) -> Result<Self, String> {
        let root = path
            .canonicalize()
            .map_err(|_| "未能讀取所選資料夾，請檢查 macOS 檔案權限。".to_string())?;
        let root_text = root
            .to_str()
            .ok_or("所選資料夾的路徑編碼不受支援。")?
            .to_string();
        let file = open_root(&root_text)?;
        // An existing vault has its own .obsidian directory. Do not turn a
        // mistaken selection of Home, Documents or the disk into a broad grant.
        let config = open_child(&file, ".obsidian", true).map_err(|_| {
            "請選取 Obsidian 資料庫的根目錄，其中必須包含 .obsidian 資料夾。".to_string()
        })?;
        if !config
            .metadata()
            .map_err(|_| "未能確認 Obsidian 資料庫。")?
            .is_dir()
        {
            return Err("請選取有效的 Obsidian 資料庫資料夾。".to_string());
        }
        let metadata = file.metadata().map_err(|_| "未能確認資料庫權限。")?;
        Ok(Self {
            root: root_text,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn open(&self) -> Result<File, String> {
        let file = open_root(&self.root)?;
        let metadata = file.metadata().map_err(|_| "未能讀取資料庫狀態。")?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(
                "Obsidian 資料庫的位置或內容身分已變更，請重新選取資料夾授權。".to_string(),
            );
        }
        Ok(file)
    }

    pub fn verify(&self) -> Result<(), String> {
        let root = self.open()?;
        let mut remaining = 1;
        // Opening the root is not sufficient on macOS: enumeration also checks
        // that the current process has the user's Files and Folders permission.
        directory_names(&root, &mut remaining)?;
        Ok(())
    }
}

fn open_root(path: &str) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| open_error(&error))
}

/// The folder picker's access lasts only until JARVIS quits; after that a
/// vault under Desktop or Documents needs the Files and Folders permission,
/// and macOS answers EPERM until it is given. Re-selecting the folder cannot
/// fix that for longer than one launch, so only a vault that is really gone
/// is sent back to the picker.
fn open_error(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => "macOS 未允許 JARVIS 讀取此資料夾。請到「系統設定 > 私隱與安全性 > 檔案與資料夾」允許 JARVIS 存取桌面或文件資料夾；連接設定會保留。".to_string(),
        Some(libc::ENOENT) | Some(libc::ENOTDIR) => {
            "找不到 Obsidian 資料庫；資料夾可能已移動或重新命名，請重新選取。".to_string()
        }
        _ => "未能開啟已授權的 Obsidian 資料庫，請重新選取資料夾或檢查 macOS 權限。".to_string(),
    }
}

fn open_child(parent: &File, name: &str, directory: bool) -> Result<File, String> {
    let name = CString::new(name).map_err(|_| "筆記路徑無效。")?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent remains alive, name is NUL terminated, and ownership of a
    // successful descriptor is transferred exactly once to File.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err("未能讀取筆記；檔案可能已移動、沒有權限或屬於符號連結。".to_string());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Return validated relative Markdown components without normalizing away
/// `..`, repeated separators or hidden names.
pub fn note_components(path: &str) -> Result<Vec<&str>, String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\\') || path.contains('\0') {
        return Err("請提供資料庫內的相對 Markdown 筆記路徑。".to_string());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > MAX_DEPTH
        || parts.iter().any(|p| p.is_empty() || p.starts_with('.'))
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        || !parts
            .last()
            .is_some_and(|p| p.to_ascii_lowercase().ends_with(".md"))
    {
        return Err(
            "只可讀取已授權資料庫內的 Markdown 筆記；不支援隱藏檔案或跨資料夾路徑。".to_string(),
        );
    }
    Ok(parts)
}

fn note_file(root: &File, path: &str) -> Result<File, String> {
    let parts = note_components(path)?;
    let device = root.metadata().map_err(|_| "未能確認資料庫權限。")?.dev();
    let mut parent = root.try_clone().map_err(|_| "未能開啟資料庫。")?;
    for (index, component) in parts.iter().enumerate() {
        parent = open_child(&parent, component, index + 1 < parts.len())?;
        if parent.metadata().map_err(|_| "未能確認筆記權限。")?.dev() != device {
            return Err("不支援讀取資料庫內另外掛載的磁碟。".to_string());
        }
    }
    let metadata = parent.metadata().map_err(|_| "未能讀取筆記狀態。")?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("只支援一般 Markdown 檔案，不支援連結或特殊檔案。".to_string());
    }
    if metadata.len() > MAX_NOTE_BYTES as u64 {
        return Err("筆記超過 1 MiB，請縮小內容後再試。".to_string());
    }
    Ok(parent)
}

fn read_file_bounded(file: File, limit: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "未能讀取筆記內容。")?;
    if bytes.len() > limit {
        return Err("筆記超過 1 MiB，請縮小內容後再試。".to_string());
    }
    String::from_utf8(bytes).map_err(|_| "筆記不是有效的 UTF-8 文字。".to_string())
}

pub fn read_note(grant: &VaultGrant, path: &str) -> Result<String, String> {
    let root = grant.open()?;
    read_file_bounded(note_file(&root, path)?, MAX_NOTE_BYTES)
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: only this guard owns the non-null stream.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn directory_names(dir: &File, remaining: &mut usize) -> Result<Vec<String>, String> {
    // openat(".") creates an independent file description; dup would share the
    // directory offset and make repeated scans unexpectedly empty.
    let handle = open_child(dir, ".", true)?;
    use std::os::fd::IntoRawFd;
    let fd = handle.into_raw_fd();
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err("未能列出 Obsidian 筆記，請檢查 macOS 檔案權限。".to_string());
    }
    let stream = DirectoryStream(raw);
    let mut names = Vec::new();
    while *remaining > 0 {
        // SAFETY: fdopendir owns the descriptor; the name is copied before the
        // next call to readdir invalidates its storage.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_str()
            .ok();
        if let Some(name) = name {
            if name == "." || name == ".." {
                continue;
            }
            *remaining -= 1;
            if !name.starts_with('.') {
                names.push(name.to_string());
            }
        } else {
            *remaining -= 1;
        }
    }
    names.sort();
    Ok(names)
}

fn walk(
    dir: &File,
    prefix: &str,
    depth: usize,
    remaining: &mut usize,
    paths: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= MAX_DEPTH || *remaining == 0 {
        return Ok(());
    }
    let device = dir.metadata().map_err(|_| "未能確認資料庫權限。")?.dev();
    for name in directory_names(dir, remaining)? {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // Filter special files before opening them. The subsequent no-follow
        // open and metadata check remain necessary: this preliminary check is
        // only a filter, not the boundary that protects against path races.
        let Ok(c_name) = CString::new(name.as_str()) else {
            continue;
        };
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let checked = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                c_name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if checked != 0 {
            continue;
        }
        let kind = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
        if kind != libc::S_IFDIR && (kind != libc::S_IFREG || note_components(&path).is_err()) {
            continue;
        }
        // Open, then inspect. Symlinks and inaccessible entries are skipped.
        let Ok(child) = open_child(dir, &name, false) else {
            continue;
        };
        let Ok(metadata) = child.metadata() else {
            continue;
        };
        if metadata.dev() != device {
            continue;
        }
        if metadata.is_dir() {
            walk(&child, &path, depth + 1, remaining, paths)?;
        } else if metadata.is_file()
            && metadata.nlink() == 1
            && metadata.len() <= MAX_NOTE_BYTES as u64
            && note_components(&path).is_ok()
        {
            paths.push(path);
        }
    }
    Ok(())
}

pub fn list_notes(grant: &VaultGrant) -> Result<Vec<String>, String> {
    let root = grant.open()?;
    let mut paths = Vec::new();
    let mut remaining = MAX_SCAN_ENTRIES;
    walk(&root, "", 0, &mut remaining, &mut paths)?;
    paths.sort();
    Ok(paths)
}

pub fn search_notes(grant: &VaultGrant, query: &str) -> Result<Vec<ObsidianNote>, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 500 {
        return Err("請輸入 1 至 500 個字元的搜尋內容。".to_string());
    }
    let query = query.to_lowercase();
    let root = grant.open()?;
    let mut paths = Vec::new();
    let mut remaining = MAX_SCAN_ENTRIES;
    walk(&root, "", 0, &mut remaining, &mut paths)?;
    paths.sort();
    let mut read_bytes = 0;
    let mut matches = Vec::new();
    for path in paths {
        let Ok(file) = note_file(&root, &path) else {
            continue;
        };
        let size = file
            .metadata()
            .map(|m| m.len() as usize)
            .unwrap_or(MAX_NOTE_BYTES);
        if read_bytes + size > MAX_SCAN_BYTES {
            break;
        }
        let Ok(content) = read_file_bounded(file, MAX_NOTE_BYTES.min(MAX_SCAN_BYTES - read_bytes))
        else {
            continue;
        };
        read_bytes += content.len();
        let matched_line = content
            .lines()
            .find(|line| line.to_lowercase().contains(&query));
        if path.to_lowercase().contains(&query) || matched_line.is_some() {
            let snippet = matched_line
                .unwrap_or(&content)
                .chars()
                .take(MAX_SNIPPET_CHARS)
                .collect();
            matches.push(ObsidianNote {
                path,
                content: Some(snippet),
            });
            if matches.len() == MAX_SEARCH_RESULTS {
                break;
            }
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A macOS privacy denial is fixed in System Settings, not by picking the
    /// folder again: re-selecting only lasts until the next launch, which is
    /// the reconnect loop. Only a vault that is really gone asks to re-select.
    #[test]
    fn a_privacy_denial_points_at_system_settings_not_at_reconnecting() {
        for code in [libc::EPERM, libc::EACCES] {
            let message = open_error(&std::io::Error::from_raw_os_error(code));
            assert!(message.contains("檔案與資料夾"), "{code}: {message}");
            assert!(!message.contains("重新選取"), "{code}: {message}");
        }
        assert!(open_error(&std::io::Error::from_raw_os_error(libc::ENOENT)).contains("重新選取"));
    }
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("jarvis-vault-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(path.join(".obsidian")).unwrap();
            Self(path)
        }
        fn grant(&self) -> VaultGrant {
            VaultGrant::authorize(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_explicit_vault_and_relative_markdown_are_accepted() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.0.join("ordinary")).unwrap();
        assert!(VaultGrant::authorize(&fixture.0.join("ordinary")).is_err());
        for path in [
            "../outside.md",
            "/outside.md",
            ".obsidian/secret.md",
            "a/.private.md",
            "a//note.md",
            "a/../note.md",
            "a\\b.md",
            "image.png",
        ] {
            assert!(note_components(path).is_err(), "accepted {path}");
        }
        assert!(note_components("公司資料/會議記錄.MD").is_ok());
    }

    #[test]
    fn hidden_nonmarkdown_symlink_hardlink_and_oversize_files_are_excluded() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let outside = Fixture::new();
        std::fs::write(outside.0.join("private.md"), "private").unwrap();
        std::fs::write(fixture.0.join("normal.md"), "會議資料").unwrap();
        std::fs::write(fixture.0.join(".obsidian/secret.md"), "secret").unwrap();
        std::fs::write(fixture.0.join("image.png"), "image").unwrap();
        symlink(outside.0.join("private.md"), fixture.0.join("linked.md")).unwrap();
        symlink(&outside.0, fixture.0.join("linked-folder")).unwrap();
        std::fs::hard_link(outside.0.join("private.md"), fixture.0.join("hard.md")).unwrap();
        let oversized = File::create(fixture.0.join("large.md")).unwrap();
        oversized.set_len(MAX_NOTE_BYTES as u64 + 1).unwrap();
        let grant = fixture.grant();
        assert_eq!(list_notes(&grant).unwrap(), ["normal.md"]);
        for path in [
            "linked.md",
            "linked-folder/private.md",
            "hard.md",
            "large.md",
        ] {
            assert!(read_note(&grant, path).is_err());
        }
    }

    #[test]
    fn search_reads_nested_utf8_notes_and_bounds_results() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.0.join("會議")).unwrap();
        std::fs::write(
            fixture.0.join("會議/記錄.md"),
            "第一行\n香港公司會議\n最後一行",
        )
        .unwrap();
        let grant = fixture.grant();
        let results = search_notes(&grant, "香港").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "會議/記錄.md");
        assert_eq!(results[0].content.as_deref(), Some("香港公司會議"));
        assert!(search_notes(&grant, "  ").is_err());
        for index in 0..60 {
            std::fs::write(fixture.0.join(format!("{index}.md")), "matching").unwrap();
        }
        assert_eq!(
            search_notes(&grant, "matching").unwrap().len(),
            MAX_SEARCH_RESULTS
        );
    }

    #[test]
    fn replacing_selected_root_does_not_reuse_the_grant() {
        let fixture = Fixture::new();
        let grant = fixture.grant();
        let moved = fixture.0.with_extension("moved");
        std::fs::rename(&fixture.0, &moved).unwrap();
        std::fs::create_dir_all(fixture.0.join(".obsidian")).unwrap();
        assert!(grant.verify().is_err());
        std::fs::remove_dir_all(moved).unwrap();
    }
}
