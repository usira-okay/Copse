use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use super::target::DeleteTarget;
use crate::{git, repository::Repository};

const CANNOT_CANONICALIZE: &str = "無法正規化路徑";
const CANNOT_READ_ROOT: &str = "無法讀取搜尋根目錄";
const CANNOT_READ_DIRECTORY: &str = "無法讀取目錄";
const CANNOT_READ_ENTRY: &str = "無法讀取目錄項目";
const CANNOT_READ_METADATA: &str = "無法讀取路徑資訊";
const CANNOT_READ_MARKER: &str = "無法讀取 Git 標記";
const INVALID_MARKER: &str = "Git 標記必須是一般檔案或目錄";
const CANNOT_DISCOVER_REPOSITORY: &str = "無法探索儲存庫";
const CANNOT_FIND_MAIN: &str = "無法確認受保護的主 worktree";
const REPOSITORY_CHANGED: &str = "探索期間儲存庫歸屬已改變";
const OUTSIDE_ROOT: &str = "位於搜尋根目錄外";
const SYMLINK_PATH: &str = "路徑經過搜尋根目錄內的符號連結";
const NOT_DIRECTORY: &str = "worktree 路徑不是目錄";
const CURRENT_DIRECTORY_ERROR: &str = "無法取得目前目錄";
const MAIN_WORKTREE: &str = "保護主 worktree";
const BARE_WORKTREE: &str = "排除 bare 儲存庫";
const CURRENT_WORKTREE: &str = "保護包含啟動目錄或有效工作目錄的 worktree";
const LOCKED_WORKTREE: &str = "排除已鎖定的 worktree";
const PRUNABLE_WORKTREE: &str = "排除失效的 worktree 登錄";
const MISSING_WORKTREE: &str = "排除不存在的 worktree";
const GIT_MARKER: &str = ".git";

/// 搜尋根目錄內可刪除的 worktree，以及排除原因與探索問題。
#[derive(Debug)]
pub struct Discovery {
    pub targets: Vec<DeleteTarget>,
    pub excluded: Vec<String>,
    pub errors: Vec<String>,
    pub root: PathBuf,
}

/// 探索搜尋根目錄內的 Git worktree，並保護主工作樹與兩個工作目錄。
pub fn discover(root: &Path, protected_dirs: &[PathBuf], verbose: bool) -> Result<Discovery> {
    let root = canonical_path(root)?;
    let protected_dirs: Vec<_> = protected_dirs
        .iter()
        .map(|directory| canonical_path(directory))
        .collect::<Result<_>>()?;
    let mut root_entries =
        Some(fs::read_dir(&root).with_context(|| format!("{CANNOT_READ_ROOT}: {root:?}"))?);
    let mut discovery = Discovery {
        targets: Vec::new(),
        excluded: Vec::new(),
        errors: Vec::new(),
        root: root.clone(),
    };
    let mut repositories = HashSet::new();
    let mut pending = vec![root.clone()];

    while let Some(directory) = pending.pop() {
        if directory != root {
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => continue,
                Err(error) => {
                    discovery
                        .errors
                        .push(format!("{CANNOT_READ_METADATA}: {directory:?}: {error}"));
                    continue;
                }
            }
        }
        let marker = directory.join(GIT_MARKER);
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.is_file() || metadata.is_dir() => {
                if let Err(error) = discover_repository(
                    &directory,
                    &protected_dirs,
                    verbose,
                    &mut repositories,
                    &mut discovery,
                ) {
                    discovery.errors.push(format!(
                        "{CANNOT_DISCOVER_REPOSITORY}: {directory:?}: {error:#}"
                    ));
                }
                continue;
            }
            Ok(_) => {
                discovery
                    .errors
                    .push(format!("{INVALID_MARKER}: {marker:?}"));
                continue;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                discovery
                    .errors
                    .push(format!("{CANNOT_READ_MARKER}: {marker:?}: {error}"));
                continue;
            }
        }

        let entries = if directory == root {
            root_entries.take().expect("搜尋根目錄僅處理一次")
        } else {
            match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    discovery
                        .errors
                        .push(format!("{CANNOT_READ_DIRECTORY}: {directory:?}: {error}"));
                    continue;
                }
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    discovery
                        .errors
                        .push(format!("{CANNOT_READ_ENTRY}: {directory:?}: {error}"));
                    continue;
                }
            };
            if entry.file_name() == GIT_MARKER {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(entry.path()),
                Ok(_) => {}
                Err(error) => discovery.errors.push(format!(
                    "{CANNOT_READ_METADATA}: {:?}: {error}",
                    entry.path()
                )),
            }
        }
    }

    discovery.targets.sort_by(|left, right| {
        left.repository
            .root
            .cmp(&right.repository.root)
            .then(left.path.cmp(&right.path))
    });
    discovery.targets.dedup_by(|left, right| {
        left.repository.common_dir == right.repository.common_dir && left.path == right.path
    });
    discovery.excluded.sort();
    discovery.excluded.dedup();
    discovery.errors.sort();
    discovery.errors.dedup();
    Ok(discovery)
}

/// 正規化路徑並移除 Windows 的延伸路徑前綴。
pub fn canonical_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path)
        .map(git::strip_unc_prefix_path)
        .with_context(|| format!("{CANNOT_CANONICALIZE}: {path:?}"))
}

/// 驗證目標位於搜尋根目錄內，且不經由根目錄內的目錄符號連結。
pub fn ensure_within_root(path: &Path, root: &Path) -> Result<PathBuf> {
    let canonical_root = canonical_path(root)?;
    let current_directory = std::env::current_dir().context(CURRENT_DIRECTORY_ERROR)?;
    let absolute_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        current_directory.join(root)
    };
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_directory.join(path)
    };
    let relative = absolute_path
        .strip_prefix(&absolute_root)
        .or_else(|_| absolute_path.strip_prefix(&canonical_root))
        .with_context(|| format!("{OUTSIDE_ROOT}: {path:?}"))?;
    let mut traversed = canonical_root.clone();
    for component in relative.components() {
        match component {
            Component::Normal(name) => traversed.push(name),
            Component::CurDir => continue,
            Component::ParentDir if traversed != canonical_root => {
                traversed.pop();
            }
            _ => bail!("{OUTSIDE_ROOT}: {path:?}"),
        }
        let metadata = fs::symlink_metadata(&traversed)
            .with_context(|| format!("{CANNOT_READ_METADATA}: {traversed:?}"))?;
        if metadata.file_type().is_symlink() {
            bail!("{SYMLINK_PATH}: {path:?}");
        }
    }
    let canonical = canonical_path(path)?;
    if !canonical.starts_with(&canonical_root) {
        bail!("{OUTSIDE_ROOT}: {path:?}");
    }
    if !fs::metadata(&canonical)
        .with_context(|| format!("{CANNOT_READ_METADATA}: {canonical:?}"))?
        .is_dir()
    {
        bail!("{NOT_DIRECTORY}: {path:?}");
    }
    Ok(canonical)
}

fn discover_repository(
    directory: &Path,
    protected_dirs: &[PathBuf],
    verbose: bool,
    repositories: &mut HashSet<PathBuf>,
    discovery: &mut Discovery,
) -> Result<()> {
    let common_dir = canonical_path(&git::get_repo_common_dir_at(directory, verbose)?)?;
    if !repositories.insert(common_dir.clone()) {
        return Ok(());
    }
    let initial_worktrees = git::list_worktrees_at(directory, verbose)?;
    let main = initial_worktrees
        .first()
        .filter(|worktree| worktree.is_main)
        .context(CANNOT_FIND_MAIN)?;
    let root = canonical_path(Path::new(&main.path)).context(CANNOT_FIND_MAIN)?;
    if canonical_path(&git::get_repo_common_dir_at(&root, verbose)?)? != common_dir {
        bail!(REPOSITORY_CHANGED);
    }
    // 後續命令固定使用不會列入刪除的主 worktree，避免依賴待刪除目錄。
    let worktrees = git::list_worktrees_at(&root, verbose)?;
    let listed_main = worktrees
        .first()
        .filter(|worktree| worktree.is_main)
        .context(CANNOT_FIND_MAIN)?;
    if canonical_path(Path::new(&listed_main.path))? != root {
        bail!(REPOSITORY_CHANGED);
    }
    let repository = Repository { common_dir, root };

    for worktree in worktrees {
        let reason = if worktree.is_main {
            Some(MAIN_WORKTREE)
        } else if worktree.is_bare {
            Some(BARE_WORKTREE)
        } else if worktree.locked.is_some() {
            Some(LOCKED_WORKTREE)
        } else if worktree.prunable.is_some() {
            Some(PRUNABLE_WORKTREE)
        } else {
            None
        };
        if let Some(reason) = reason {
            discovery
                .excluded
                .push(format!("{reason}: {:?}", worktree.path));
            continue;
        }
        let raw_path = Path::new(&worktree.path);
        match fs::symlink_metadata(raw_path) {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                discovery
                    .excluded
                    .push(format!("{MISSING_WORKTREE}: {raw_path:?}"));
                continue;
            }
            Err(error) => {
                discovery
                    .errors
                    .push(format!("{CANNOT_READ_METADATA}: {raw_path:?}: {error}"));
                continue;
            }
        }
        let path = match canonical_path(raw_path) {
            Ok(path) => path,
            Err(error) => {
                discovery.errors.push(format!("{error:#}"));
                continue;
            }
        };
        if !path.starts_with(&discovery.root) {
            discovery
                .excluded
                .push(format!("{OUTSIDE_ROOT}: {raw_path:?}"));
            continue;
        }
        if let Err(error) = ensure_within_root(raw_path, &discovery.root) {
            discovery.errors.push(format!("{error:#}"));
            continue;
        }
        if protected_dirs
            .iter()
            .any(|directory| directory.starts_with(&path))
        {
            discovery
                .excluded
                .push(format!("{CURRENT_WORKTREE}: {raw_path:?}"));
            continue;
        }
        discovery.targets.push(DeleteTarget {
            repository: repository.clone(),
            worktree,
            path,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::test_support::TestSandbox;

    use super::{canonical_path, discover, ensure_within_root};

    #[test]
    fn discovers_two_repositories_and_deduplicates_shared_repository() {
        let sandbox = TestSandbox::new();
        let alpha = sandbox.repo("repos/alpha");
        let beta = sandbox.repo("repos/beta");
        let first = sandbox.worktree(&alpha, "batch/alpha-one", "shared");
        let second = sandbox.worktree(&alpha, "batch/alpha-two", "second");
        let third = sandbox.worktree(&beta, "batch/beta", "shared");
        sandbox.worktree(&alpha, "outside/alpha", "outside");
        let root = sandbox.root.join("batch");

        let discovery = discover(&root, std::slice::from_ref(&alpha), false).unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.root, canonical_path(&root).unwrap());
        assert_eq!(discovery.targets.len(), 3);
        assert_eq!(discovery.targets[0].path, canonical_path(&first).unwrap());
        assert_eq!(discovery.targets[1].path, canonical_path(&second).unwrap());
        assert_eq!(discovery.targets[2].path, canonical_path(&third).unwrap());
        assert_eq!(
            discovery.targets[0].repository.root,
            canonical_path(&alpha).unwrap()
        );
        assert_eq!(
            discovery.targets[2].repository.root,
            canonical_path(&beta).unwrap()
        );
        assert_ne!(
            discovery.targets[0].repository.common_dir,
            discovery.targets[2].repository.common_dir
        );
    }

    #[test]
    fn excludes_main_current_directory_descendants_and_locked_worktrees() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let current = sandbox.worktree(&repo, "batch/current", "current");
        let locked = sandbox.worktree(&repo, "batch/locked", "locked");
        let available = sandbox.worktree(&repo, "batch/available", "available");
        let original = current.join("source/nested");
        fs::create_dir_all(&original).unwrap();
        assert!(
            sandbox
                .git(
                    &repo,
                    &[
                        "worktree",
                        "lock",
                        "--reason",
                        "測試鎖定",
                        locked.to_str().unwrap()
                    ]
                )
                .status
                .success()
        );

        let discovery = discover(&sandbox.root, std::slice::from_ref(&original), false).unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.targets.len(), 1);
        assert_eq!(
            discovery.targets[0].path,
            canonical_path(&available).unwrap()
        );
        assert!(
            discovery
                .excluded
                .iter()
                .any(|reason| reason.contains("主 worktree"))
        );
        assert!(
            discovery
                .excluded
                .iter()
                .any(|reason| reason.contains("啟動目錄"))
        );
        assert!(
            discovery
                .excluded
                .iter()
                .any(|reason| reason.contains("鎖定"))
        );
    }

    #[test]
    fn excludes_missing_prunable_entries_without_losing_valid_targets() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let missing = sandbox.worktree(&repo, "batch/missing", "missing");
        let available = sandbox.worktree(&repo, "batch/available", "available");
        fs::remove_dir_all(&missing).unwrap();

        let discovery = discover(
            &sandbox.root.join("batch"),
            std::slice::from_ref(&repo),
            false,
        )
        .unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.targets.len(), 1);
        assert_eq!(
            discovery.targets[0].path,
            canonical_path(&available).unwrap()
        );
        assert!(
            discovery
                .excluded
                .iter()
                .any(|reason| { reason.contains("失效") || reason.contains("不存在") })
        );
    }

    #[test]
    fn includes_a_worktree_that_is_the_search_root() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "exact root", "feature");

        let discovery = discover(&worktree, std::slice::from_ref(&repo), false).unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.targets.len(), 1);
        assert_eq!(
            discovery.targets[0].path,
            canonical_path(&worktree).unwrap()
        );
    }

    #[test]
    fn rejects_missing_or_non_directory_search_root() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let file = sandbox.root.join("file");
        fs::write(&file, "不是目錄").unwrap();

        assert!(
            discover(
                &sandbox.root.join("missing"),
                std::slice::from_ref(&repo),
                false
            )
            .is_err()
        );
        assert!(discover(&file, std::slice::from_ref(&repo), false).is_err());
    }

    #[test]
    fn does_not_descend_into_nested_repositories_inside_an_existing_repository() {
        let sandbox = TestSandbox::new();
        let outer = sandbox.repo("outer");
        let inner = sandbox.repo("outer/inner");
        sandbox.worktree(&inner, "outer/inner-linked", "nested");

        let discovery = discover(&outer, std::slice::from_ref(&outer), false).unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert!(discovery.targets.is_empty());
    }

    #[test]
    fn reports_broken_repository_and_keeps_other_available_targets() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "batch/good", "good");
        let broken = sandbox.root.join("batch/broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join(".git"), "gitdir: /copse/missing/repository\n").unwrap();

        let discovery = discover(
            &sandbox.root.join("batch"),
            std::slice::from_ref(&repo),
            false,
        )
        .unwrap();

        assert_eq!(discovery.targets.len(), 1);
        assert_eq!(
            discovery.targets[0].path,
            canonical_path(&worktree).unwrap()
        );
        assert!(!discovery.errors.is_empty());
        assert!(
            discovery
                .errors
                .iter()
                .any(|error| error.contains("broken"))
        );
    }

    #[test]
    fn rejects_paths_outside_root_using_component_boundaries() {
        let sandbox = TestSandbox::new();
        let root = sandbox.root.join("batch");
        let outside = sandbox.root.join("batch-extra/worktree");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();

        assert!(ensure_within_root(&outside, &root).is_err());
        assert_eq!(
            ensure_within_root(&root, &root).unwrap(),
            canonical_path(&root).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn skips_directory_symlinks_and_rejects_symlink_target_paths() {
        use std::os::unix::fs::symlink;

        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "outside/feature", "feature");
        let root = sandbox.root.join("batch");
        fs::create_dir_all(&root).unwrap();
        symlink(&worktree, root.join("shortcut")).unwrap();

        let discovery = discover(&root, std::slice::from_ref(&repo), false).unwrap();

        assert!(discovery.targets.is_empty());
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert!(ensure_within_root(&root.join("shortcut"), &root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn accepts_an_explicit_symlink_root_but_rejects_inner_symlinks() {
        use std::os::unix::fs::symlink;

        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let worktree = sandbox.worktree(&repo, "batch/feature", "feature");
        let root = sandbox.root.join("batch");
        let alias = sandbox.root.join("alias");
        symlink(&root, &alias).unwrap();
        symlink(&worktree, root.join("inner")).unwrap();

        let discovery = discover(&alias, std::slice::from_ref(&repo), false).unwrap();

        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.targets.len(), 1);
        assert_eq!(
            ensure_within_root(&alias.join("feature"), &alias).unwrap(),
            canonical_path(&worktree).unwrap()
        );
        assert!(ensure_within_root(&root.join("inner"), &root).is_err());
    }
}
