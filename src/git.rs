use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Check if the current directory is inside a git repository.
pub fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the top-level directory of the git repository.
pub fn get_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git rev-parse")?;
    if !output.status.success() {
        bail!("Not inside a git repository");
    }
    let path = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in git output")?
        .trim()
        .to_string();
    Ok(PathBuf::from(path))
}

/// Represents a branch or tag reference.
#[derive(Debug, Clone)]
pub struct GitRef {
    pub name: String,
    pub display: String,
    pub ref_type: GitRefType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRefType {
    LocalBranch,
    RemoteBranch,
    LocalTag,
    RemoteTag,
}

impl GitRefType {
    pub fn sort_order(&self) -> u8 {
        match self {
            GitRefType::LocalBranch => 0,
            GitRefType::RemoteBranch => 1,
            GitRefType::LocalTag => 2,
            GitRefType::RemoteTag => 3,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            GitRefType::LocalBranch => "Local Branch",
            GitRefType::RemoteBranch => "Remote Branch",
            GitRefType::LocalTag => "Local Tag",
            GitRefType::RemoteTag => "Remote Tag",
        }
    }
}

/// Get all branches and tags sorted by: local branch > remote branch > local tag > remote tag.
pub fn get_all_refs(verbose: bool) -> Result<Vec<GitRef>> {
    let mut refs = Vec::new();

    // Local branches
    let output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .output()
        .context("Failed to list local branches")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let name = line.trim().to_string();
            if !name.is_empty() {
                refs.push(GitRef {
                    display: format!("[Local Branch] {}", name),
                    name,
                    ref_type: GitRefType::LocalBranch,
                });
            }
        }
    }

    // Remote branches
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .output()
        .context("Failed to list remote branches")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let name = line.trim().to_string();
            if !name.is_empty() && !name.contains("HEAD") {
                refs.push(GitRef {
                    display: format!("[Remote Branch] {}", name),
                    name,
                    ref_type: GitRefType::RemoteBranch,
                });
            }
        }
    }

    // Local tags
    let output = Command::new("git")
        .args(["tag", "--list"])
        .output()
        .context("Failed to list tags")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let name = line.trim().to_string();
            if !name.is_empty() {
                refs.push(GitRef {
                    display: format!("[Local Tag] {}", name),
                    name,
                    ref_type: GitRefType::LocalTag,
                });
            }
        }
    }

    // Remote tags (tags from ls-remote that aren't in local tags)
    let local_tags: Vec<String> = refs
        .iter()
        .filter(|r| r.ref_type == GitRefType::LocalTag)
        .map(|r| r.name.clone())
        .collect();

    let output = Command::new("git")
        .args(["ls-remote", "--tags"])
        .output()
        .context("Failed to list remote tags")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Format: <sha>\trefs/tags/<name>
            if let Some(ref_path) = line.split('\t').nth(1) {
                let name = ref_path
                    .strip_prefix("refs/tags/")
                    .unwrap_or(ref_path)
                    .to_string();
                // Skip annotated tag dereferences
                if name.ends_with("^{}") {
                    continue;
                }
                if !local_tags.contains(&name) {
                    refs.push(GitRef {
                        display: format!("[Remote Tag] {}", name),
                        name,
                        ref_type: GitRefType::RemoteTag,
                    });
                }
            }
        }
    }

    // Sort by type order
    refs.sort_by(|a, b| {
        a.ref_type
            .sort_order()
            .cmp(&b.ref_type.sort_order())
            .then_with(|| a.name.cmp(&b.name))
    });

    if verbose {
        eprintln!("[DEBUG] Found {} refs:", refs.len());
        for r in &refs {
            eprintln!("  {} ({})", r.name, r.ref_type.label());
        }
    }

    Ok(refs)
}

pub fn local_branch_name_for_ref(ref_name: &str, ref_type: GitRefType) -> Option<String> {
    match ref_type {
        GitRefType::RemoteBranch => {
            let name = ref_name.trim();
            if name.is_empty() || name == "HEAD" {
                return None;
            }

            let local_name: String = name
                .split('/')
                .skip(1)
                .collect::<Vec<_>>()
                .join("/");

            if local_name.is_empty() || local_name == name {
                None
            } else {
                Some(local_name)
            }
        }
        _ => None,
    }
}

pub fn create_worktree_from_ref(
    path: &Path,
    ref_name: &str,
    ref_type: GitRefType,
    verbose: bool,
) -> Result<()> {
    if let Some(branch_name) = local_branch_name_for_ref(ref_name, ref_type) {
        let branch_exists = Command::new("git")
            .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch_name}")])
            .status()
            .context("Failed to check whether the local branch exists")?
            .success();

        if branch_exists {
            return create_worktree(path, &branch_name, verbose);
        }

        return create_worktree_new_branch(path, &branch_name, ref_name, verbose);
    }

    create_worktree(path, ref_name, verbose)
}

/// Represents a worktree entry.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_main: bool,
    pub is_bare: bool,
    /// Git 登錄的鎖定原因；空字串表示未提供原因。
    pub locked: Option<String>,
    /// Git 登錄已失效的原因；空字串表示未提供原因。
    pub prunable: Option<String>,
}

/// List all worktrees.
pub fn list_worktrees(verbose: bool) -> Result<Vec<Worktree>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to list worktrees")?;

    if !output.status.success() {
        bail!(
            "Failed to list worktrees: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();
    let mut is_bare = false;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = path.to_string();
            current_branch = String::new();
            is_bare = false;
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch
                .strip_prefix("refs/heads/")
                .unwrap_or(branch)
                .to_string();
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() && !current_path.is_empty() {
            worktrees.push(Worktree {
                path: current_path.clone(),
                branch: current_branch.clone(),
                is_main: worktrees.is_empty(),
                is_bare,
                locked: None,
                prunable: None,
            });
            current_path = String::new();
            current_branch = String::new();
            is_bare = false;
        }
    }

    // Handle last entry if no trailing newline
    if !current_path.is_empty() {
        worktrees.push(Worktree {
            path: current_path,
            branch: current_branch,
            is_main: worktrees.is_empty(),
            is_bare,
            locked: None,
            prunable: None,
        });
    }

    if verbose {
        eprintln!("[DEBUG] Found {} worktrees:", worktrees.len());
        for wt in &worktrees {
            eprintln!(
                "  {} (branch: {}, main: {}, bare: {})",
                wt.path, wt.branch, wt.is_main, wt.is_bare
            );
        }
    }

    Ok(worktrees)
}

const BATCH_GIT_LIST_FAILED: &str = "無法讀取 worktree 登錄";
const BATCH_GIT_PARSE_FAILED: &str = "worktree 登錄格式不完整或無法安全解析";
const BATCH_GIT_UTF8_FAILED: &str = "Git 輸出含無法安全解析的 UTF-8";
const BATCH_GIT_REMOVE_FAILED: &str = "無法刪除 worktree";
const BATCH_GIT_BRANCH_FAILED: &str = "無法刪除本地分支";
const BATCH_GIT_COMMON_DIR_FAILED: &str = "無法取得儲存庫共用 Git 目錄";
const BATCH_GIT_CANONICALIZE_FAILED: &str = "無法正規化儲存庫共用 Git 目錄";
const BATCH_GIT_STATUS_FAILED: &str = "無法確認 worktree 的變更狀態";
const BATCH_GIT_VERSION_FAILED: &str = "無法取得 Git 版本";
const BATCH_GIT_VERSION_REQUIRED: &str =
    "批次刪除需要 Git 2.36 以上版本；請升級 Git，其他操作仍可使用";
const BATCH_GIT_PATH_FAILED: &str = "Git 回傳無效路徑";
const BATCH_GIT_MIN_VERSION: (u64, u64) = (2, 36);
const BATCH_GIT_LOCAL_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
];

fn batch_git_command(repo_dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(repo_dir);
    // 呼叫端可能由 Git hook 啟動；清除儲存庫區域環境，避免跨 repo 命令誤用來源 repo。
    for name in BATCH_GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    command
}

/// 從指定儲存庫讀取完整且未經路徑跳脫的 worktree 登錄。
pub fn list_worktrees_at(repo_dir: &Path, verbose: bool) -> Result<Vec<Worktree>> {
    let output = batch_git_command(repo_dir)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .with_context(|| format!("{BATCH_GIT_LIST_FAILED}：{repo_dir:?}"))?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_LIST_FAILED}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let worktrees = parse_worktrees_z(&output.stdout)?;
    if verbose {
        eprintln!("[DEBUG] {repo_dir:?}：{} worktrees", worktrees.len());
    }
    Ok(worktrees)
}

fn parse_worktrees_z(output: &[u8]) -> Result<Vec<Worktree>> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let output = std::str::from_utf8(output).context(BATCH_GIT_UTF8_FAILED)?;
    let records = output
        .strip_suffix("\0\0")
        .context(BATCH_GIT_PARSE_FAILED)?;
    let mut worktrees = Vec::new();
    for record in records.split("\0\0") {
        let mut fields = record.split('\0');
        let path = fields
            .next()
            .and_then(|field| field.strip_prefix("worktree "))
            .context(BATCH_GIT_PARSE_FAILED)?;
        ensure!(
            !path.is_empty() && Path::new(path).is_absolute(),
            BATCH_GIT_PARSE_FAILED
        );
        let mut worktree = Worktree {
            path: path.to_string(),
            branch: String::new(),
            is_main: worktrees.is_empty(),
            is_bare: false,
            locked: None,
            prunable: None,
        };
        let mut has_head = false;
        let mut has_branch = false;
        let mut detached = false;
        for field in fields {
            if let Some(head) = field.strip_prefix("HEAD ") {
                ensure!(
                    !has_head
                        && !head.is_empty()
                        && head.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    BATCH_GIT_PARSE_FAILED
                );
                has_head = true;
            } else if let Some(branch) = field.strip_prefix("branch ") {
                ensure!(!has_branch, BATCH_GIT_PARSE_FAILED);
                let branch = branch
                    .strip_prefix("refs/heads/")
                    .filter(|branch| !branch.is_empty())
                    .context(BATCH_GIT_PARSE_FAILED)?;
                worktree.branch = branch.to_string();
                has_branch = true;
            } else if field == "detached" {
                ensure!(!detached, BATCH_GIT_PARSE_FAILED);
                detached = true;
            } else if field == "bare" {
                ensure!(!worktree.is_bare, BATCH_GIT_PARSE_FAILED);
                worktree.is_bare = true;
            } else if field == "locked" || field.starts_with("locked ") {
                ensure!(worktree.locked.is_none(), BATCH_GIT_PARSE_FAILED);
                worktree.locked = Some(field.strip_prefix("locked ").unwrap_or("").to_string());
            } else if field == "prunable" || field.starts_with("prunable ") {
                ensure!(worktree.prunable.is_none(), BATCH_GIT_PARSE_FAILED);
                worktree.prunable = Some(field.strip_prefix("prunable ").unwrap_or("").to_string());
            } else {
                bail!("{BATCH_GIT_PARSE_FAILED}：{field:?}");
            }
        }
        if worktree.is_bare {
            ensure!(
                !has_head && !has_branch && !detached,
                BATCH_GIT_PARSE_FAILED
            );
        } else {
            ensure!(has_head && has_branch != detached, BATCH_GIT_PARSE_FAILED);
        }
        worktrees.push(worktree);
    }
    Ok(worktrees)
}

/// 從指定儲存庫移除 worktree，不變更程序的目前目錄。
pub fn remove_worktree_at(repo_dir: &Path, path: &Path, force: bool, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("[DEBUG] {repo_dir:?}：git worktree remove {path:?} (force={force})");
    }
    let mut command = batch_git_command(repo_dir);
    command.args(["worktree", "remove"]);
    if force {
        command.arg("--force");
    }
    let output = command
        .arg("--")
        .arg(path)
        .output()
        .with_context(|| format!("{BATCH_GIT_REMOVE_FAILED}：{path:?}"))?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_REMOVE_FAILED}：{path:?}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// 在指定儲存庫刪除本地分支，強制模式會略過合併狀態檢查。
pub fn delete_branch_at(repo_dir: &Path, branch: &str, force: bool, verbose: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    if verbose {
        eprintln!("[DEBUG] {repo_dir:?}：git branch {flag} -- {branch:?}");
    }
    let output = batch_git_command(repo_dir)
        .args(["branch", flag, "--", branch])
        .output()
        .with_context(|| format!("{BATCH_GIT_BRANCH_FAILED}：{branch:?}"))?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_BRANCH_FAILED}：{branch:?}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// 取得指定儲存庫正規化後的共用 Git 目錄，供跨 worktree 識別儲存庫。
pub fn get_repo_common_dir_at(repo_dir: &Path, verbose: bool) -> Result<PathBuf> {
    let output = batch_git_command(repo_dir)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .with_context(|| format!("{BATCH_GIT_COMMON_DIR_FAILED}：{repo_dir:?}"))?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_COMMON_DIR_FAILED}：{repo_dir:?}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let path = decode_git_path(&output.stdout)?;
    ensure!(path.is_absolute(), BATCH_GIT_PATH_FAILED);
    let canonical = path
        .canonicalize()
        .with_context(|| format!("{BATCH_GIT_CANONICALIZE_FAILED}：{path:?}"))?;
    ensure!(canonical.to_str().is_some(), BATCH_GIT_UTF8_FAILED);
    let canonical = strip_unc_prefix_path(canonical);
    if verbose {
        eprintln!("[DEBUG] {repo_dir:?}：git common directory {canonical:?}");
    }
    Ok(canonical)
}

/// 檢查指定 worktree 的變更，包含使用者設定隱藏的未追蹤檔案與子模組變更。
pub fn worktree_has_changes_at(path: &Path, verbose: bool) -> Result<bool> {
    let output = batch_git_command(path)
        .args([
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])
        .output()
        .with_context(|| format!("{BATCH_GIT_STATUS_FAILED}：{path:?}"))?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_STATUS_FAILED}：{path:?}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let has_changes = !output.stdout.is_empty();
    if verbose {
        eprintln!("[DEBUG] {path:?}：has_changes={has_changes}");
    }
    Ok(has_changes)
}

/// 確認 Git 支援批次刪除所需的 NUL 分隔 worktree 格式。
pub fn require_batch_delete_git(verbose: bool) -> Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .context(BATCH_GIT_VERSION_FAILED)?;
    if !output.status.success() {
        bail!(
            "{BATCH_GIT_VERSION_FAILED}：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    check_batch_delete_git_version(&output.stdout)?;
    if verbose {
        eprintln!(
            "[DEBUG] {}",
            String::from_utf8_lossy(&output.stdout).trim_end()
        );
    }
    Ok(())
}

fn check_batch_delete_git_version(output: &[u8]) -> Result<()> {
    let version = std::str::from_utf8(output)
        .context(BATCH_GIT_UTF8_FAILED)?
        .strip_prefix("git version ")
        .and_then(|text| text.split_whitespace().next())
        .context(BATCH_GIT_VERSION_FAILED)?;
    let mut components = version.split('.');
    let major = components
        .next()
        .context(BATCH_GIT_VERSION_FAILED)?
        .parse::<u64>()
        .context(BATCH_GIT_VERSION_FAILED)?;
    let minor = components
        .next()
        .context(BATCH_GIT_VERSION_FAILED)?
        .parse::<u64>()
        .context(BATCH_GIT_VERSION_FAILED)?;
    ensure!(
        (major, minor) >= BATCH_GIT_MIN_VERSION,
        BATCH_GIT_VERSION_REQUIRED
    );
    Ok(())
}

fn decode_git_path(output: &[u8]) -> Result<PathBuf> {
    let path = std::str::from_utf8(output).context(BATCH_GIT_UTF8_FAILED)?;
    let path = path.strip_suffix('\n').unwrap_or(path);
    ensure!(
        !path.is_empty() && !path.contains('\0'),
        BATCH_GIT_PATH_FAILED
    );
    Ok(PathBuf::from(path))
}

/// Find the main worktree (the original repository clone) from a list of
/// worktrees, regardless of its position in the list.
fn find_main_worktree(worktrees: &[Worktree]) -> Option<&Worktree> {
    worktrees.iter().find(|wt| wt.is_main)
}

/// Get the root directory of the main worktree (the original repository
/// clone), regardless of which worktree the command is currently run from.
pub fn get_main_worktree_root(verbose: bool) -> Result<PathBuf> {
    let worktrees = list_worktrees(verbose)?;
    let main = find_main_worktree(&worktrees).context("Could not find main worktree")?;
    Ok(PathBuf::from(&main.path))
}

/// Create a worktree at the given path for the given branch.
pub fn create_worktree(path: &Path, branch: &str, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!(
            "[DEBUG] Creating worktree at {:?} for branch {}",
            path, branch
        );
    }

    let output = Command::new("git")
        .args(["worktree", "add", &path.to_string_lossy(), branch])
        .output()
        .context("Failed to create worktree")?;

    if !output.status.success() {
        bail!(
            "Failed to create worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    if verbose {
        eprintln!(
            "[DEBUG] Worktree created: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    Ok(())
}

/// Create a worktree with a new branch based on the given start point.
pub fn create_worktree_new_branch(
    path: &Path,
    new_branch: &str,
    start_point: &str,
    verbose: bool,
) -> Result<()> {
    if verbose {
        eprintln!(
            "[DEBUG] Creating worktree at {:?} with new branch {} from {}",
            path, new_branch, start_point
        );
    }

    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            new_branch,
            &path.to_string_lossy(),
            start_point,
        ])
        .output()
        .context("Failed to create worktree with new branch")?;

    if !output.status.success() {
        bail!(
            "Failed to create worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Remove a worktree.
pub fn remove_worktree(path: &str, force: bool, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("[DEBUG] Removing worktree at {} (force: {})", path, force);
    }

    let mut args = vec!["worktree", "remove", path];
    if force {
        args.push("--force");
    }

    let output = Command::new("git")
        .args(&args)
        .output()
        .context("Failed to remove worktree")?;

    if !output.status.success() {
        bail!(
            "Failed to remove worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Delete a branch.
pub fn delete_branch(branch: &str, force: bool, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("[DEBUG] Deleting branch {} (force: {})", branch, force);
    }

    let flag = if force { "-D" } else { "-d" };
    let output = Command::new("git")
        .args(["branch", flag, branch])
        .output()
        .context("Failed to delete branch")?;

    if !output.status.success() {
        bail!(
            "Failed to delete branch: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Check if a worktree has uncommitted changes.
pub fn worktree_has_changes(path: &str, verbose: bool) -> Result<bool> {
    if verbose {
        eprintln!("[DEBUG] Checking for uncommitted changes in {}", path);
    }

    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .context("Failed to check worktree status")?;

    if !output.status.success() {
        bail!(
            "Failed to check worktree status: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let has_changes = !String::from_utf8_lossy(&output.stdout).trim().is_empty();

    if verbose {
        eprintln!("[DEBUG] Worktree {} has changes: {}", path, has_changes);
    }

    Ok(has_changes)
}

/// Get the diff (patch) of changes in a worktree (both staged and unstaged).
pub fn get_worktree_diff(path: &str, verbose: bool) -> Result<String> {
    if verbose {
        eprintln!("[DEBUG] Getting diff from worktree at {}", path);
    }

    // Get unstaged changes
    let unstaged = Command::new("git")
        .args(["diff"])
        .current_dir(path)
        .output()
        .context("Failed to get unstaged diff")?;

    // Get staged changes
    let staged = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(path)
        .output()
        .context("Failed to get staged diff")?;

    let mut diff = String::new();
    let unstaged_str = String::from_utf8_lossy(&unstaged.stdout);
    let staged_str = String::from_utf8_lossy(&staged.stdout);

    if !unstaged_str.is_empty() {
        diff.push_str(&unstaged_str);
    }
    if !staged_str.is_empty() {
        if !diff.is_empty() {
            diff.push('\n');
        }
        diff.push_str(&staged_str);
    }

    // Also get untracked files
    let untracked = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(path)
        .output()
        .context("Failed to list untracked files")?;

    let untracked_str = String::from_utf8_lossy(&untracked.stdout);
    if !untracked_str.trim().is_empty() && verbose {
        eprintln!("[DEBUG] Untracked files in source worktree:");
        for f in untracked_str.lines() {
            eprintln!("  {}", f);
        }
    }

    Ok(diff)
}

/// Apply a diff (patch) to the current worktree.
pub fn apply_diff(diff: &str, verbose: bool) -> Result<bool> {
    if verbose {
        eprintln!("[DEBUG] Applying diff ({} bytes)", diff.len());
    }

    let mut child = Command::new("git")
        .args(["apply", "--check", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn git apply --check")?;

    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        stdin
            .write_all(diff.as_bytes())
            .context("Failed to write diff to git apply --check")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git apply --check")?;
    let can_apply_cleanly = output.status.success();

    if verbose {
        eprintln!("[DEBUG] Diff can apply cleanly: {}", can_apply_cleanly);
        if !can_apply_cleanly {
            eprintln!(
                "[DEBUG] Apply check errors: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(can_apply_cleanly)
}

/// Force apply a diff to the current worktree.
pub fn force_apply_diff(diff: &str, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("[DEBUG] Force applying diff ({} bytes)", diff.len());
    }

    let mut child = Command::new("git")
        .args(["apply", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn git apply")?;

    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        stdin
            .write_all(diff.as_bytes())
            .context("Failed to write diff to git apply")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git apply")?;

    if !output.status.success() {
        bail!(
            "Failed to apply diff: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Apply diff with 3-way merge fallback for conflict handling.
pub fn apply_diff_with_fallback(diff: &str, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!(
            "[DEBUG] Applying diff with 3-way merge fallback ({} bytes)",
            diff.len()
        );
    }

    let mut child = Command::new("git")
        .args(["apply", "--3way", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn git apply --3way")?;

    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        stdin
            .write_all(diff.as_bytes())
            .context("Failed to write diff to git apply --3way")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git apply --3way")?;

    if verbose {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprintln!("[DEBUG] Apply --3way output: {}", stderr);
        }
    }

    if !output.status.success() {
        bail!(
            "Failed to apply diff (some conflicts may remain): {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Strip UNC extended-length path prefix (\\?\) that `std::fs::canonicalize`
/// produces on Windows. PowerShell's Set-Location and many other tools do not
/// support these paths.
pub fn strip_unc_prefix(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

/// Strip UNC prefix from a `PathBuf`, returning a cleaned `PathBuf`.
pub fn strip_unc_prefix_path(path: PathBuf) -> PathBuf {
    let s = path.display().to_string();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Copy untracked files from source worktree to current directory.
pub fn copy_untracked_files(source_path: &str, verbose: bool) -> Result<()> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(source_path)
        .output()
        .context("Failed to list untracked files")?;

    let files = String::from_utf8_lossy(&output.stdout);
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    for file in files.lines() {
        let file = file.trim();
        if file.is_empty() {
            continue;
        }
        let source = Path::new(source_path).join(file);
        let dest = current_dir.join(file);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {:?}", parent))?;
        }

        if verbose {
            eprintln!("[DEBUG] Copying untracked file: {} -> {:?}", file, dest);
        }

        std::fs::copy(&source, &dest)
            .with_context(|| format!("Failed to copy {:?} to {:?}", source, dest))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{find_main_worktree, Worktree};

    fn make_worktree(path: &str, branch: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.to_string(),
            branch: branch.to_string(),
            is_main,
            is_bare: false,
            locked: None,
            prunable: None,
        }
    }

    #[test]
    fn finds_main_worktree_when_not_first_in_list() {
        let worktrees = vec![
            make_worktree("/repo/worktree/feature-a", "feature-a", false),
            make_worktree("/repo", "main", true),
            make_worktree("/repo/worktree/feature-b", "feature-b", false),
        ];

        let main = find_main_worktree(&worktrees).expect("main worktree should be found");

        assert_eq!(main.path, "/repo");
        assert!(main.is_main);
    }

    #[test]
    fn returns_none_when_no_main_worktree_present() {
        let worktrees = vec![
            make_worktree("/repo/worktree/feature-a", "feature-a", false),
            make_worktree("/repo/worktree/feature-b", "feature-b", false),
        ];

        assert!(find_main_worktree(&worktrees).is_none());
    }

    #[test]
    fn batch_parser_preserves_special_characters_and_status_reasons() {
        let output = concat!(
            "worktree /repo main \n\0HEAD 0123456789abcdef\0branch refs/heads/main\0\0",
            "worktree /worktrees/\"tab\tnewline\ntrailing \n\0",
            "HEAD abcdef0123456789\0branch refs/heads/feature/topic\0",
            "locked 管理員\n鎖定 \0prunable gitdir file points to non-existent location\0\0",
            "worktree /worktrees/detached\0HEAD abcdef\0detached\0locked\0\0",
            "worktree /bare.git\0bare\0\0",
        );
        let path_prefix = if cfg!(windows) { "C:" } else { "" };
        let output = output.replace("worktree /", &format!("worktree {path_prefix}/"));

        let entries = super::parse_worktrees_z(output.as_bytes()).unwrap();

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].path, format!("{path_prefix}/repo main \n"));
        assert_eq!(entries[0].branch, "main");
        assert!(entries[0].is_main);
        assert_eq!(
            entries[1].path,
            format!("{path_prefix}/worktrees/\"tab\tnewline\ntrailing \n")
        );
        assert_eq!(entries[1].branch, "feature/topic");
        assert!(!entries[1].is_main);
        assert_eq!(entries[1].locked.as_deref(), Some("管理員\n鎖定 "));
        assert_eq!(
            entries[1].prunable.as_deref(),
            Some("gitdir file points to non-existent location")
        );
        assert!(entries[2].branch.is_empty());
        assert_eq!(entries[2].locked.as_deref(), Some(""));
        assert!(entries[2].prunable.is_none());
        assert!(entries[3].is_bare);
        assert!(entries[3].branch.is_empty());
    }

    #[test]
    fn batch_parser_rejects_invalid_utf8_and_incomplete_records() {
        let malformed: &[&[u8]] = &[
            b"worktree /repo\xff\0HEAD abcdef\0branch refs/heads/main\0\0",
            b"worktree /repo\0HEAD abcdef\0branch refs/heads/main\0",
            b"HEAD abcdef\0branch refs/heads/main\0\0",
            b"worktree \0HEAD abcdef\0branch refs/heads/main\0\0",
            b"worktree /repo\0HEAD abcdef\0\0",
            b"worktree /repo\0branch refs/heads/main\0\0",
            b"worktree /repo\0HEAD abcdef\0branch refs/heads/main\0detached\0\0",
            b"worktree /repo\0HEAD abcdef\0branch refs/heads/main\0branch refs/heads/other\0\0",
        ];

        for output in malformed {
            assert!(super::parse_worktrees_z(output).is_err(), "{output:?}");
        }
        assert!(super::parse_worktrees_z(b"").unwrap().is_empty());
    }

    #[test]
    fn batch_git_version_requires_236_and_accepts_platform_suffixes() {
        for supported in [
            "git version 2.36.0\n",
            "git version 2.46.0.windows.1\n",
            "git version 2.39.5 (Apple Git-154)\n",
            "git version 3.0.0\n",
        ] {
            super::check_batch_delete_git_version(supported.as_bytes()).unwrap();
        }

        for unsupported in [
            "git version 2.35.9\n",
            "git version 1.99.0\n",
            "git version unknown\n",
            "git version 2\n",
            "unrecognized version output\n",
        ] {
            assert!(super::check_batch_delete_git_version(unsupported.as_bytes()).is_err());
        }
    }

    #[test]
    fn batch_git_path_removes_only_the_command_terminator() {
        assert_eq!(
            super::decode_git_path(b"/repo trailing \n\n").unwrap(),
            std::path::PathBuf::from("/repo trailing \n")
        );
        assert_eq!(
            super::decode_git_path(b"/repo trailing \n").unwrap(),
            std::path::PathBuf::from("/repo trailing ")
        );
        assert!(super::decode_git_path(b"/repo\xff\n").is_err());
        assert!(super::decode_git_path(b"\n").is_err());
    }

    #[test]
    fn batch_git_operations_keep_two_repositories_with_the_same_branch_separate() {
        let sandbox = crate::test_support::TestSandbox::new();
        let first = sandbox.repo("first");
        let second = sandbox.repo("second");
        let first_worktree = sandbox.worktree(&first, "worktrees/first", "shared-topic");
        let second_worktree = sandbox.worktree(&second, "worktrees/second", "shared-topic");

        let first_common = super::get_repo_common_dir_at(&first, false).unwrap();
        assert_eq!(
            first_common,
            super::get_repo_common_dir_at(&first_worktree, false).unwrap()
        );
        assert_ne!(
            first_common,
            super::get_repo_common_dir_at(&second, false).unwrap()
        );
        assert!(
            super::list_worktrees_at(&first, false)
                .unwrap()
                .iter()
                .any(|worktree| std::path::Path::new(&worktree.path) == first_worktree)
        );
        assert!(
            !super::list_worktrees_at(&first, false)
                .unwrap()
                .iter()
                .any(|worktree| std::path::Path::new(&worktree.path) == second_worktree)
        );

        super::remove_worktree_at(&first, &first_worktree, false, false).unwrap();
        super::delete_branch_at(&first, "shared-topic", false, false).unwrap();

        assert!(!first_worktree.exists());
        assert!(second_worktree.exists());
        assert!(
            !std::process::Command::new("git")
                .args(["show-ref", "--verify", "--quiet", "refs/heads/shared-topic"])
                .current_dir(&first)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            sandbox
                .git(
                    &second,
                    &["show-ref", "--verify", "--quiet", "refs/heads/shared-topic"]
                )
                .status
                .success()
        );
    }

    #[test]
    fn batch_git_force_removes_dirty_worktrees_but_never_unlocks_them() {
        let sandbox = crate::test_support::TestSandbox::new();
        let repo = sandbox.repo("repo");
        let dirty = sandbox.worktree(&repo, "worktrees/dirty", "dirty-topic");
        let locked = sandbox.worktree(&repo, "worktrees/locked", "locked-topic");
        std::fs::write(dirty.join("untracked.txt"), "保留直到強制刪除").unwrap();
        let output = sandbox.git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "測試保護",
                locked.to_str().unwrap(),
            ],
        );
        assert!(output.status.success());

        assert!(super::remove_worktree_at(&repo, &dirty, false, false).is_err());
        assert!(dirty.exists());
        super::remove_worktree_at(&repo, &dirty, true, false).unwrap();
        assert!(!dirty.exists());
        assert!(super::remove_worktree_at(&repo, &locked, true, false).is_err());
        assert!(locked.exists());
        let entries = super::list_worktrees_at(&repo, false).unwrap();
        let entry = entries
            .iter()
            .find(|entry| std::path::Path::new(&entry.path) == locked)
            .unwrap();
        assert_eq!(entry.locked.as_deref(), Some("測試保護"));
    }

    #[test]
    fn batch_git_branch_cleanup_uses_the_requested_merge_safety() {
        let sandbox = crate::test_support::TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "worktrees/unmerged", "unmerged-topic");
        std::fs::write(worktree.join("feature.txt"), "尚未合併").unwrap();
        assert!(
            sandbox
                .git(&worktree, &["add", "feature.txt"])
                .status
                .success()
        );
        assert!(
            sandbox
                .git(&worktree, &["commit", "-m", "feature"])
                .status
                .success()
        );
        super::remove_worktree_at(&repo, &worktree, false, false).unwrap();

        assert!(super::delete_branch_at(&repo, "unmerged-topic", false, false).is_err());
        assert!(
            sandbox
                .git(
                    &repo,
                    &[
                        "show-ref",
                        "--verify",
                        "--quiet",
                        "refs/heads/unmerged-topic"
                    ]
                )
                .status
                .success()
        );
        super::delete_branch_at(&repo, "unmerged-topic", true, false).unwrap();
        assert!(
            !std::process::Command::new("git")
                .args([
                    "show-ref",
                    "--verify",
                    "--quiet",
                    "refs/heads/unmerged-topic"
                ])
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn batch_git_detects_untracked_files_even_when_user_configuration_hides_them() {
        let sandbox = crate::test_support::TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "worktrees/hidden-untracked", "hidden-untracked");
        assert!(
            sandbox
                .git(&repo, &["config", "status.showUntrackedFiles", "no"])
                .status
                .success()
        );
        assert!(!super::worktree_has_changes_at(&worktree, false).unwrap());

        std::fs::write(worktree.join("hidden-untracked.txt"), "不可隱藏的變更").unwrap();

        assert!(super::worktree_has_changes_at(&worktree, false).unwrap());
    }

    #[test]
    fn batch_git_ignores_foreign_repository_environment() {
        const CHILD_FLAG: &str = "COPSE_BATCH_TEST_FOREIGN_ENV";
        const TARGET_REPO: &str = "COPSE_BATCH_TEST_TARGET_REPO";
        const TARGET_WORKTREE: &str = "COPSE_BATCH_TEST_TARGET_WORKTREE";
        const TEST_NAME: &str = "git::tests::batch_git_ignores_foreign_repository_environment";
        const BRANCH: &str = "shared-topic";

        if std::env::var_os(CHILD_FLAG).is_some() {
            let repo = std::path::PathBuf::from(std::env::var_os(TARGET_REPO).unwrap());
            let worktree = std::path::PathBuf::from(std::env::var_os(TARGET_WORKTREE).unwrap());
            let expected_common =
                super::strip_unc_prefix_path(repo.join(".git").canonicalize().unwrap());
            assert_eq!(
                super::get_repo_common_dir_at(&repo, false).unwrap(),
                expected_common
            );
            let entries = super::list_worktrees_at(&repo, false).unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(
                std::path::Path::new(&entries[0].path)
                    .canonicalize()
                    .unwrap(),
                repo.canonicalize().unwrap()
            );
            assert!(entries.iter().any(|entry| {
                std::path::Path::new(&entry.path).canonicalize().unwrap()
                    == worktree.canonicalize().unwrap()
            }));
            assert!(!super::worktree_has_changes_at(&repo, false).unwrap());
            assert!(super::worktree_has_changes_at(&worktree, false).unwrap());

            super::remove_worktree_at(&repo, &worktree, true, false).unwrap();
            super::delete_branch_at(&repo, BRANCH, true, false).unwrap();
            assert!(!worktree.exists());
            return;
        }

        let sandbox = crate::test_support::TestSandbox::new();
        let foreign_repo = sandbox.repo("foreign");
        let target_repo = sandbox.repo("target");
        let foreign_worktree = sandbox.worktree(&foreign_repo, "worktrees/foreign", BRANCH);
        let target_worktree = sandbox.worktree(&target_repo, "worktrees/target", BRANCH);
        std::fs::write(
            target_worktree.join("untracked.txt"),
            "只允許刪除目標儲存庫",
        )
        .unwrap();

        // 子程序隔離環境污染，避免改動平行測試使用的全域環境。
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_FLAG, "1")
            .env(TARGET_REPO, &target_repo)
            .env(TARGET_WORKTREE, &target_worktree)
            .env("GIT_DIR", foreign_repo.join(".git"))
            .env("GIT_COMMON_DIR", foreign_repo.join(".git"))
            .env("GIT_WORK_TREE", &foreign_repo)
            .env("GIT_INDEX_FILE", foreign_repo.join(".git/index"))
            .env("GIT_OBJECT_DIRECTORY", foreign_repo.join(".git/objects"))
            .env(
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                foreign_repo.join(".git/objects"),
            )
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.worktree")
            .env("GIT_CONFIG_VALUE_0", &foreign_repo)
            .env("GIT_NAMESPACE", "foreign")
            .current_dir(&foreign_repo)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "子程序驗證失敗：{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(foreign_worktree.exists());
        assert!(!target_worktree.exists());
        assert!(
            sandbox
                .git(
                    &foreign_repo,
                    &["show-ref", "--verify", "--quiet", "refs/heads/shared-topic"]
                )
                .status
                .success()
        );
        assert!(
            !std::process::Command::new("git")
                .args(["show-ref", "--verify", "--quiet", "refs/heads/shared-topic"])
                .current_dir(&target_repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn strips_remote_prefix_when_resolving_remote_branch_name() {
        let branch = super::local_branch_name_for_ref("origin/feature/mask-code", super::GitRefType::RemoteBranch);

        assert_eq!(branch, Some("feature/mask-code".to_string()));
    }

    #[test]
    fn create_worktree_from_remote_branch_checks_out_named_branch() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("copse-remote-branch-{unique}"));
        let remote = base.join("remote.git");
        let repo = base.join("repo");
        let worktree_path = base.join("feature-worktree");

        std::fs::create_dir_all(&base).unwrap();

        let original_dir = std::env::current_dir().unwrap();

        let status = std::process::Command::new("git")
            .args(["init", "--bare", remote.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args([
                "-c",
                "init.defaultBranch=master",
                "clone",
                remote.to_str().unwrap(),
                repo.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["config", "user.name", "Copse Test"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["config", "user.email", "copse@example.com"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        std::fs::write(repo.join("README.md"), "hello\n").unwrap();

        let status = std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["checkout", "-b", "feature/mask-code"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["push", "-u", "origin", "feature/mask-code"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["checkout", "master"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .args(["branch", "-D", "feature/mask-code"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        std::env::set_current_dir(&repo).unwrap();

        super::create_worktree_from_ref(
            &worktree_path,
            "origin/feature/mask-code",
            super::GitRefType::RemoteBranch,
            false,
        )
        .unwrap();

        let output = std::process::Command::new("git")
            .args(["-C", worktree_path.to_str().unwrap(), "branch", "--show-current"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "feature/mask-code");

        std::env::set_current_dir(original_dir).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }
}
