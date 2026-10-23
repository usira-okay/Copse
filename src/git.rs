use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod worktree_source;
pub use worktree_source::WorktreeSource;

const REPO_ROOT_FAILED: &str = "無法取得 worktree 根目錄，請確認目前位於 Git 儲存庫內";
const CREATE_REF_INVALID: &str = "來源必須是本地分支、遠端追蹤分支、標籤、HEAD 或唯一的 commit SHA";
const CREATE_REF_MISSING: &str = "找不到來源 ref 或來源不是 commit";
const CREATE_REF_AMBIGUOUS: &str =
    "來源名稱有歧義，請指定完整 refs/heads/、refs/remotes/ 或 refs/tags/ 名稱";
const CREATE_REF_LIST_FAILED: &str = "無法列出本地儲存庫的 refs";
const CREATE_BRANCH_INVALID: &str = "新分支名稱無效";
const CREATE_BRANCH_EXISTS: &str = "本地同名分支已存在，無法建立新分支";
const CREATE_BRANCH_CHECK_FAILED: &str = "無法確認本地分支狀態";
const CREATE_WORKTREE_FAILED: &str = "無法建立 worktree";
const CREATE_PATH_EXISTS: &str = "建立 worktree 的目標路徑已存在";
const CREATE_PATH_CHECK_FAILED: &str = "無法確認建立 worktree 的實體路徑";
const SINGLE_LINE_PATH_REQUIRED: &str = "worktree 路徑必須是有效 UTF-8 且不得包含 CR 或 LF";
const HEADS_PREFIX: &str = "refs/heads/";
const REMOTES_PREFIX: &str = "refs/remotes/";
const TAGS_PREFIX: &str = "refs/tags/";

/// 取得目前工作目錄所屬 worktree 的根目錄。
pub fn get_repo_root() -> Result<PathBuf> {
    get_repo_root_at(Path::new("."), false)
}

/// 取得指定目錄所屬 worktree 的正規化根目錄。
pub fn get_repo_root_at(repo_dir: &Path, verbose: bool) -> Result<PathBuf> {
    let output = command_at(repo_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context(REPO_ROOT_FAILED)?;
    ensure!(
        output.status.success(),
        "{REPO_ROOT_FAILED}：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = decode_git_path(&output.stdout)?
        .canonicalize()
        .context(REPO_ROOT_FAILED)?;
    let root = strip_unc_prefix_path(root);
    if verbose {
        eprintln!("[DEBUG] worktree 根目錄：{root:?}");
    }
    Ok(root)
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

/// 依本地分支、遠端分支、本地標籤、遠端標籤排序互動清單。
pub fn get_all_refs(verbose: bool) -> Result<Vec<GitRef>> {
    let mut refs = Vec::new();

    // Local branches
    let output = command_at(Path::new("."))
        .args(["branch", "--format=%(refname)"])
        .output()
        .context("Failed to list local branches")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let name = line.strip_prefix(HEADS_PREFIX).unwrap_or(line).to_string();
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
    let output = command_at(Path::new("."))
        .args(["branch", "-r", "--format=%(refname)%00%(symref)"])
        .output()
        .context("Failed to list remote branches")?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let (reference, symbolic_target) =
                line.split_once('\0').context(CREATE_REF_LIST_FAILED)?;
            let name = reference
                .strip_prefix(REMOTES_PREFIX)
                .unwrap_or(reference)
                .to_string();
            if !name.is_empty() && symbolic_target.is_empty() {
                refs.push(GitRef {
                    display: format!("[Remote Branch] {}", name),
                    name,
                    ref_type: GitRefType::RemoteBranch,
                });
            }
        }
    }

    // Local tags
    let output = command_at(Path::new("."))
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

    let output = command_at(Path::new("."))
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

/// 從遠端追蹤分支名稱移除遠端前綴，保留分支本身的子路徑。
pub fn local_branch_name_for_ref(ref_name: &str, ref_type: GitRefType) -> Option<String> {
    match ref_type {
        GitRefType::RemoteBranch => {
            let name = ref_name.trim();
            if name.is_empty() || name == "HEAD" {
                return None;
            }

            let local_name: String = name.split('/').skip(1).collect::<Vec<_>>().join("/");

            if local_name.is_empty() || local_name == name {
                None
            } else {
                Some(local_name)
            }
        }
        _ => None,
    }
}

/// 僅解析已存在於本地的 ref 或 commit，不接受 revision expression，也不執行 fetch。
pub fn resolve_worktree_source_at(repo_dir: &Path, reference: &str) -> Result<WorktreeSource> {
    ensure!(
        !reference.is_empty()
            && !reference.starts_with('-')
            && !reference.contains(['\0', '\r', '\n']),
        CREATE_REF_INVALID
    );
    if reference == "HEAD" {
        return detached_source(repo_dir, reference, reference);
    }

    let output = command_at(repo_dir)
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "--",
            HEADS_PREFIX,
            REMOTES_PREFIX,
            TAGS_PREFIX,
        ])
        .output()
        .context(CREATE_REF_LIST_FAILED)?;
    ensure!(
        output.status.success(),
        "{CREATE_REF_LIST_FAILED}：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let refs = std::str::from_utf8(&output.stdout).context(BATCH_GIT_UTF8_FAILED)?;
    let matches: Vec<&str> = refs
        .lines()
        .filter(|name| {
            *name == reference
                || (!reference.starts_with("refs/")
                    && [HEADS_PREFIX, REMOTES_PREFIX, TAGS_PREFIX]
                        .iter()
                        .any(|prefix| name.strip_prefix(*prefix) == Some(reference)))
        })
        .collect();
    ensure!(matches.len() <= 1, "{CREATE_REF_AMBIGUOUS}：{reference:?}");
    let Some(full_ref) = matches.first().copied() else {
        ensure!(
            reference.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{CREATE_REF_MISSING}：{reference:?}"
        );
        return detached_source(repo_dir, reference, reference);
    };
    let commit = resolve_commit_at(repo_dir, full_ref)?;
    if let Some(branch) = full_ref.strip_prefix(HEADS_PREFIX) {
        return Ok(WorktreeSource {
            reference: full_ref.to_string(),
            commit,
            directory_name: branch.to_string(),
            local_branch: Some(branch.to_string()),
            track_remote: false,
        });
    }
    if let Some(remote_branch) = full_ref.strip_prefix(REMOTES_PREFIX) {
        let branch = local_branch_name_for_ref(remote_branch, GitRefType::RemoteBranch)
            .context(CREATE_REF_INVALID)?;
        validate_branch_name_at(repo_dir, &branch)?;
        return Ok(WorktreeSource {
            reference: full_ref.to_string(),
            commit,
            directory_name: branch.clone(),
            local_branch: Some(branch),
            track_remote: true,
        });
    }
    Ok(WorktreeSource {
        reference: full_ref.to_string(),
        commit,
        directory_name: full_ref
            .strip_prefix(TAGS_PREFIX)
            .context(CREATE_REF_INVALID)?
            .to_string(),
        local_branch: None,
        track_remote: false,
    })
}

fn detached_source(
    repo_dir: &Path,
    reference: &str,
    directory_name: &str,
) -> Result<WorktreeSource> {
    let commit = resolve_commit_at(repo_dir, reference)?;
    Ok(WorktreeSource {
        reference: commit.clone(),
        commit,
        directory_name: directory_name.to_string(),
        local_branch: None,
        track_remote: false,
    })
}

fn resolve_commit_at(repo_dir: &Path, reference: &str) -> Result<String> {
    let output = command_at(repo_dir)
        .args([
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ])
        .output()
        .context(CREATE_REF_MISSING)?;
    ensure!(
        output.status.success(),
        "{CREATE_REF_MISSING}：{reference:?}：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commit = std::str::from_utf8(&output.stdout)
        .context(BATCH_GIT_UTF8_FAILED)?
        .trim_end_matches('\n');
    ensure!(
        !commit.is_empty() && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        CREATE_REF_MISSING
    );
    Ok(commit.to_string())
}

fn validate_branch_name_at(repo_dir: &Path, branch: &str) -> Result<()> {
    ensure!(
        !branch.is_empty() && !branch.starts_with('-') && !branch.contains('\0'),
        CREATE_BRANCH_INVALID
    );
    let output = command_at(repo_dir)
        .args(["check-ref-format", &format!("{HEADS_PREFIX}{branch}")])
        .output()
        .context(CREATE_BRANCH_INVALID)?;
    ensure!(
        output.status.success(),
        "{CREATE_BRANCH_INVALID}：{branch:?}"
    );
    Ok(())
}

fn ensure_new_branch_at(repo_dir: &Path, branch: &str) -> Result<()> {
    validate_branch_name_at(repo_dir, branch)?;
    let output = command_at(repo_dir)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            "--",
            &format!("{HEADS_PREFIX}{branch}"),
        ])
        .output()
        .context(CREATE_BRANCH_CHECK_FAILED)?;
    match output.status.code() {
        Some(1) => Ok(()),
        Some(0) => bail!("{CREATE_BRANCH_EXISTS}：{branch:?}"),
        _ => bail!(
            "{CREATE_BRANCH_CHECK_FAILED}：{branch:?}：{}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// 確保 shell 包裝器可將切換路徑安全表示成單行標記。
pub fn ensure_single_line_path(path: &Path) -> Result<()> {
    let path = path.to_str().context(SINGLE_LINE_PATH_REQUIRED)?;
    ensure!(!path.contains(['\r', '\n']), SINGLE_LINE_PATH_REQUIRED);
    Ok(())
}

/// 從已解析來源建立 worktree，明確控制分支附著、追蹤與 detached HEAD。
pub fn create_worktree_at(
    repo_dir: &Path,
    path: &Path,
    source: &WorktreeSource,
    new_branch: Option<&str>,
    verbose: bool,
) -> Result<()> {
    ensure_single_line_path(path)?;
    ensure!(
        !path.try_exists().context(CREATE_PATH_EXISTS)? && !path.is_symlink(),
        "{CREATE_PATH_EXISTS}：{path:?}"
    );
    // 既存祖先可能透過符號連結指向含換行的路徑，必須在建立分支或目錄前確認。
    let mut ancestor = path.parent().context(CREATE_PATH_CHECK_FAILED)?;
    while !ancestor.try_exists().context(CREATE_PATH_CHECK_FAILED)? {
        ancestor = ancestor.parent().context(CREATE_PATH_CHECK_FAILED)?;
    }
    ensure_single_line_path(&ancestor.canonicalize().context(CREATE_PATH_CHECK_FAILED)?)?;
    let mut command = command_at(repo_dir);
    command.args(["worktree", "add"]);
    let reference = if let Some(branch) = new_branch {
        ensure_new_branch_at(repo_dir, branch)?;
        command.args(["--no-track", "-b", branch]);
        &source.commit
    } else if source.track_remote {
        let branch = source.local_branch.as_deref().context(CREATE_REF_INVALID)?;
        ensure_new_branch_at(repo_dir, branch)?;
        command.args(["--track", "-b", branch]);
        &source.reference
    } else if let Some(branch) = &source.local_branch {
        branch
    } else {
        command.arg("--detach");
        &source.commit
    };
    let output = command
        .arg("--")
        .arg(path)
        .arg(reference)
        .output()
        .with_context(|| format!("{CREATE_WORKTREE_FAILED}：{path:?}"))?;
    ensure!(
        output.status.success(),
        "{CREATE_WORKTREE_FAILED}：{path:?}：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if verbose {
        eprintln!("[DEBUG] 已建立 worktree：{path:?}，來源：{reference:?}");
    }
    Ok(())
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

/// 以 NUL 分隔格式取得目前儲存庫的完整 worktree 登錄。
pub fn list_worktrees(verbose: bool) -> Result<Vec<Worktree>> {
    list_worktrees_at(Path::new("."), verbose)
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
const BATCH_GIT_VERSION_REQUIRED: &str = "所有操作需要 Git 2.36 以上版本；請升級 Git";
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

/// 在指定儲存庫執行 Git，隔離外部儲存庫環境並關閉一般互動輸入。
pub fn command_at(repo_dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repo_dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "");
    // 呼叫端可能由 Git hook 啟動；清除儲存庫區域環境，避免跨 repo 命令誤用來源 repo。
    for name in BATCH_GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    command
}

/// 從指定儲存庫讀取完整且未經路徑跳脫的 worktree 登錄。
pub fn list_worktrees_at(repo_dir: &Path, verbose: bool) -> Result<Vec<Worktree>> {
    let output = command_at(repo_dir)
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
    let mut command = command_at(repo_dir);
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
    let output = command_at(repo_dir)
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
    let output = command_at(repo_dir)
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
    let output = command_at(path)
        .args([
            "--no-optional-locks",
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

/// 確認所有操作共用的 Git 版本與 NUL 分隔 worktree 格式可用。
pub fn require_batch_delete_git(verbose: bool) -> Result<()> {
    let output = command_at(Path::new("."))
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

/// 檢查 worktree 的變更，讀取狀態時不刷新 Git index。
pub fn worktree_has_changes(path: &str, verbose: bool) -> Result<bool> {
    worktree_has_changes_at(Path::new(path), verbose)
}

/// Get the diff (patch) of changes in a worktree (both staged and unstaged).
pub fn get_worktree_diff(path: &str, verbose: bool) -> Result<String> {
    if verbose {
        eprintln!("[DEBUG] Getting diff from worktree at {}", path);
    }

    // Get unstaged changes
    let unstaged = command_at(Path::new("."))
        .args(["diff"])
        .current_dir(path)
        .output()
        .context("Failed to get unstaged diff")?;

    // Get staged changes
    let staged = command_at(Path::new("."))
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
    let untracked = command_at(Path::new("."))
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

    let mut child = command_at(Path::new("."))
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

    let mut child = command_at(Path::new("."))
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

    let mut child = command_at(Path::new("."))
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
    let output = command_at(Path::new("."))
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
        let branch = super::local_branch_name_for_ref(
            "origin/feature/mask-code",
            super::GitRefType::RemoteBranch,
        );

        assert_eq!(branch, Some("feature/mask-code".to_string()));
    }

    #[test]
    fn create_worktree_from_remote_branch_checks_out_named_branch() {
        let sandbox = crate::test_support::TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree_path = sandbox.root.join("feature-worktree");
        sandbox.git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
        sandbox.git(
            &repo,
            &[
                "update-ref",
                "refs/remotes/origin/feature/mask-code",
                "HEAD",
            ],
        );
        let source = super::resolve_worktree_source_at(&repo, "origin/feature/mask-code").unwrap();
        super::create_worktree_at(&repo, &worktree_path, &source, None, false).unwrap();

        let output = sandbox.git(&worktree_path, &["branch", "--show-current"]);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "feature/mask-code"
        );
        let upstream = sandbox.git(
            &worktree_path,
            &["rev-parse", "--symbolic-full-name", "@{upstream}"],
        );
        assert_eq!(
            String::from_utf8(upstream.stdout).unwrap().trim(),
            "refs/remotes/origin/feature/mask-code"
        );
    }
}
