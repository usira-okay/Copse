use crate::git::{self, Worktree};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

const INVALID_PATH: &str = "無法正規化 worktree 路徑";
const INVALID_SELECTOR: &str = "必須擇一指定 worktree 路徑或本地分支";
const UNKNOWN_TARGET: &str = "找不到指定的 worktree";
const AMBIGUOUS_TARGET: &str = "指定分支對應多個 worktree，請改用明確路徑";

/// 取得實際路徑，統一 Windows 延伸路徑前綴。
pub fn canonical_path(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map(git::strip_unc_prefix_path)
        .with_context(|| format!("{INVALID_PATH}：{path:?}"))
}

/// 依完整根目錄或精確本地分支選取唯一登錄項目。
pub fn resolve<'a>(
    entries: &'a [Worktree],
    directory: &Path,
    path: Option<&Path>,
    branch: Option<&str>,
) -> Result<&'a Worktree> {
    ensure!(path.is_some() != branch.is_some(), INVALID_SELECTOR);
    let path = path
        .map(|path| canonical_path(&directory.join(path)))
        .transpose()?;
    let mut matches = entries.iter().filter(|entry| {
        if let Some(branch) = branch {
            entry.branch == branch
        } else {
            // 已失效登錄沒有實際目錄，但仍可在 list 中查閱。
            canonical_path(Path::new(&entry.path)).ok().as_ref() == path.as_ref()
        }
    });
    let selected = matches.next().context(UNKNOWN_TARGET)?;
    ensure!(matches.next().is_none(), AMBIGUOUS_TARGET);
    Ok(selected)
}
