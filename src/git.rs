use anyhow::{Context, Result, bail};
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
