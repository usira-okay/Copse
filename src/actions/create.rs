use anyhow::{Context, Result, bail};
use console::style;
use dialoguer::Input;
use std::path::{Path, PathBuf};

use crate::{fuzzy_select::fuzzy_select, git};

const NO_REFS: &str = "找不到可建立 worktree 的分支或標籤。";
const SELECT_REFERENCE: &str = "選擇建立 worktree 的分支或標籤";
const OPERATION_CANCELLED: &str = "已取消操作。";
const SELECT_CREATE_MODE: &str = "選擇建立 worktree 的方式";
const CREATE_NEW_BRANCH: &str = "🌿  從來源建立新分支與 worktree";
const CREATE_DIRECTLY: &str = "📂  直接從來源建立 worktree";
const ENTER_NEW_BRANCH: &str = "輸入新分支名稱";
const BRANCH_NAME_REQUIRED: &str = "分支名稱不可為空白";
const CURRENT_DIR_FAILED: &str = "無法取得目前工作目錄";
const MAIN_WORKTREE_REQUIRED: &str = "找不到主要 worktree";
const REPO_NAME_REQUIRED: &str = "無法取得儲存庫名稱";
const TARGET_PATH_FAILED: &str = "無法取得建立後的 worktree 路徑";
const CD_MARKER: &str = "COPSE_CD:";

/// 收集互動輸入後，使用參數模式共用的建立流程。
pub fn run(verbose: bool, absolute_path: bool) -> Result<()> {
    let refs = git::get_all_refs(verbose)?;
    if refs.is_empty() {
        println!("{}", style(NO_REFS).yellow());
        return Ok(());
    }
    let display_items: Vec<&str> = refs
        .iter()
        .map(|reference| reference.display.as_str())
        .collect();
    let Some(selection) = fuzzy_select(SELECT_REFERENCE, &display_items)? else {
        println!("{}", style(OPERATION_CANCELLED).yellow());
        return Ok(());
    };
    let selected_ref = &refs[selection];
    let Some(create_choice) =
        fuzzy_select(SELECT_CREATE_MODE, &[CREATE_NEW_BRANCH, CREATE_DIRECTLY])?
    else {
        println!("{}", style(OPERATION_CANCELLED).yellow());
        return Ok(());
    };
    let new_branch = if create_choice == 0 {
        let name: String = Input::new().with_prompt(ENTER_NEW_BRANCH).interact_text()?;
        if name.trim().is_empty() {
            bail!(BRANCH_NAME_REQUIRED);
        }
        Some(name)
    } else {
        None
    };
    // 使用完整 ref 保留使用者在互動清單中選擇的來源種類。
    let reference = match selected_ref.ref_type {
        git::GitRefType::LocalBranch => format!("refs/heads/{}", selected_ref.name),
        git::GitRefType::RemoteBranch => format!("refs/remotes/{}", selected_ref.name),
        git::GitRefType::LocalTag | git::GitRefType::RemoteTag => {
            format!("refs/tags/{}", selected_ref.name)
        }
    };
    execute(
        &std::env::current_dir().context(CURRENT_DIR_FAILED)?,
        &reference,
        new_branch.as_deref(),
        verbose,
        absolute_path,
    )
}

/// 使用明確參數建立 worktree；路徑配置沿用主要 worktree，顯示以有效工作目錄為基準。
pub fn execute(
    repo_dir: &Path,
    reference: &str,
    new_branch: Option<&str>,
    verbose: bool,
    absolute_path: bool,
) -> Result<()> {
    let source = git::resolve_worktree_source_at(repo_dir, reference)?;
    let worktrees = git::list_worktrees_at(repo_dir, verbose)?;
    let main_root = Path::new(
        &worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .context(MAIN_WORKTREE_REQUIRED)?
            .path,
    );
    let repo_name = main_root
        .file_name()
        .and_then(|name| name.to_str())
        .context(REPO_NAME_REQUIRED)?;
    let branch_name = new_branch.unwrap_or(&source.directory_name);
    let worktree_path = build_worktree_path(main_root, repo_name, branch_name);
    git::ensure_single_line_path(&worktree_path)?;
    let display_path = if absolute_path {
        worktree_path.clone()
    } else {
        pathdiff_relative(repo_dir, &worktree_path)
    };
    git::create_worktree_at(repo_dir, &worktree_path, &source, new_branch, verbose)?;
    println!("已建立 worktree：{}", style(display_path.display()).green());
    let target = worktree_path.canonicalize().context(TARGET_PATH_FAILED)?;
    let target = git::strip_unc_prefix_path(target);
    git::ensure_single_line_path(&target)?;
    println!("{CD_MARKER}{}", target.display());
    Ok(())
}

/// 以有效工作目錄計算可直接使用的相對路徑。
fn pathdiff_relative(base: &std::path::Path, target: &std::path::Path) -> PathBuf {
    // Windows 的延伸路徑前綴會干擾元件比較，先統一成一般路徑。
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let base = git::strip_unc_prefix_path(base);
    let target_abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    };

    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target_abs.components().collect();

    let common_len = base_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();

    for _ in common_len..base_components.len() {
        result.push("..");
    }

    for component in &target_components[common_len..] {
        result.push(component);
    }

    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

fn build_worktree_path(repo_root: &Path, repo_name: &str, branch_name: &str) -> PathBuf {
    let parent_dir = repo_root
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));
    // 與相對路徑基準使用一致的正規化方式。
    let parent_dir = std::fs::canonicalize(&parent_dir)
        .map(git::strip_unc_prefix_path)
        .unwrap_or(parent_dir);

    parent_dir
        .join("worktree")
        .join(repo_name)
        .join(sanitize_branch_name(branch_name))
}

/// 沿用既有目錄命名規則，替換各平台檔案系統不接受的字元。
fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{build_worktree_path, pathdiff_relative};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_repo_root(repo_name: &str) -> (PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("copse-test-{unique}"));
        let repo_root = temp_root.join(repo_name);
        std::fs::create_dir_all(&repo_root).unwrap();
        (temp_root, repo_root)
    }

    #[test]
    fn builds_worktree_path_under_worktree_directory() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");

        let path = build_worktree_path(&repo_root, "ProjectA", "feature-1");

        assert_eq!(
            path,
            temp_root
                .join("worktree")
                .join("ProjectA")
                .join("feature-1")
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn sanitizes_branch_name_in_worktree_path() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");

        let path = build_worktree_path(&repo_root, "ProjectA", "feature/test:1");

        assert_eq!(
            path,
            temp_root
                .join("worktree")
                .join("ProjectA")
                .join("feature_test_1")
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn computes_relative_path_for_nested_worktree_directory() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");
        let worktree_path = temp_root
            .join("worktree")
            .join("ProjectA")
            .join("feature-1");

        let relative = pathdiff_relative(&repo_root, &worktree_path);

        assert_eq!(relative, PathBuf::from("../worktree/ProjectA/feature-1"));

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
