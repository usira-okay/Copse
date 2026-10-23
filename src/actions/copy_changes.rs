use crate::{git, worktree_selection};
use anyhow::{Context, Result, ensure};
use dialoguer::{Confirm, FuzzySelect, theme::ColorfulTheme};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const CURRENT_DIRECTORY_FAILED: &str = "無法取得目前工作目錄";
const NO_SOURCES: &str = "沒有其他可複製變更的 worktree。";
const SELECT_SOURCE: &str = "選擇要複製變更的來源 worktree";
const INCLUDE_UNTRACKED: &str = "是否包含未追蹤檔案？既有目的檔案不會被覆寫";
const ALLOW_THREE_WAY: &str = "直接套用失敗時是否允許三方合併？可能留下需要處理的衝突";
const CONFIRM_COPY: &str = "是否將來源變更套用到目前 worktree？";
const CANCELLED: &str = "已取消操作。";
const INVALID_SOURCE: &str = "來源必須是有效且非 bare 的 worktree 根目錄";
const DIFFERENT_WORKTREES_REQUIRED: &str = "來源與目的必須是不同的 worktree";
const SAME_REPOSITORY_REQUIRED: &str = "來源與目的 worktree 必須屬於同一個儲存庫";
const NO_CHANGES: &str = "沒有符合選項的變更可複製。";
const COPY_COMPLETED: &str = "已複製來源 worktree 的變更。";
const UNTRACKED_SOURCE_INVALID: &str = "未追蹤來源必須是一般檔案，且不得經過符號連結";
const UNTRACKED_DESTINATION_EXISTS: &str = "未追蹤檔案的目的路徑已存在";
const UNTRACKED_PARENT_INVALID: &str = "未追蹤檔案的父路徑必須是一般目錄，且不得經過符號連結";
const UNTRACKED_PATH_FAILED: &str = "無法檢查未追蹤檔案路徑";
const UNTRACKED_COPY_FAILED: &str = "無法複製未追蹤檔案";

/// 先收集全部互動選項，再使用參數模式共用的驗證與複製流程。
pub fn run(verbose: bool) -> Result<()> {
    let directory = std::env::current_dir().context(CURRENT_DIRECTORY_FAILED)?;
    let destination = git::get_repo_root_at(&directory, verbose)?;
    let sources: Vec<_> = git::list_worktrees_at(&directory, verbose)?
        .into_iter()
        .filter(|entry| {
            !entry.is_bare && entry.prunable.is_none() && Path::new(&entry.path) != destination
        })
        .collect();
    if sources.is_empty() {
        println!("{NO_SOURCES}");
        return Ok(());
    }
    let labels: Vec<_> = sources
        .iter()
        .map(|entry| format!("{:?} [{}]", entry.path, entry.branch))
        .collect();
    let Some(selection) = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(SELECT_SOURCE)
        .items(&labels)
        .default(0)
        .interact_opt()?
    else {
        println!("{CANCELLED}");
        return Ok(());
    };
    let include_untracked = Confirm::new()
        .with_prompt(INCLUDE_UNTRACKED)
        .default(false)
        .interact()?;
    let three_way = Confirm::new()
        .with_prompt(ALLOW_THREE_WAY)
        .default(false)
        .interact()?;
    if !Confirm::new()
        .with_prompt(CONFIRM_COPY)
        .default(true)
        .interact()?
    {
        println!("{CANCELLED}");
        return Ok(());
    }
    execute(
        &directory,
        &sources[selection],
        include_untracked,
        three_way,
        verbose,
    )
}

/// 複製同一儲存庫另一個 worktree 的最終內容；所有檔案衝突先於任何寫入檢查。
pub fn execute(
    directory: &Path,
    source: &git::Worktree,
    include_untracked: bool,
    three_way: bool,
    verbose: bool,
) -> Result<()> {
    ensure!(!source.is_bare && source.prunable.is_none(), INVALID_SOURCE);
    let source_path = worktree_selection::canonical_path(Path::new(&source.path))?;
    ensure!(
        git::get_repo_root_at(&source_path, verbose)? == source_path,
        INVALID_SOURCE
    );
    let destination = git::get_repo_root_at(directory, verbose)?;
    ensure!(source_path != destination, DIFFERENT_WORKTREES_REQUIRED);
    ensure!(
        git::get_repo_common_dir_at(&source_path, verbose)?
            == git::get_repo_common_dir_at(&destination, verbose)?,
        SAME_REPOSITORY_REQUIRED
    );
    let diff = git::get_worktree_diff_at(&source_path, verbose)?;
    let untracked = if include_untracked {
        git::list_untracked_files_at(&source_path, verbose)?
    } else {
        Vec::new()
    };
    // 預檢整份清單，避免後面的檔案有衝突時，前面的 patch 或檔案已經寫入。
    for relative in &untracked {
        validate_untracked_file(&source_path, &destination, relative)?;
    }
    if diff.is_empty() && untracked.is_empty() {
        println!("{NO_CHANGES}");
        return Ok(());
    }
    git::apply_worktree_diff_at(&destination, &diff, three_way, verbose)?;
    copy_untracked_files(&source_path, &destination, &untracked, verbose)?;
    println!("{COPY_COMPLETED}");
    Ok(())
}

fn validate_untracked_file(source: &Path, destination: &Path, relative: &Path) -> Result<()> {
    validate_parent_directories(source, relative, true)?;
    validate_parent_directories(destination, relative, false)?;
    let source_file = source.join(relative);
    let metadata = source_file
        .symlink_metadata()
        .with_context(|| format!("{UNTRACKED_PATH_FAILED}：{source_file:?}"))?;
    ensure!(
        metadata.file_type().is_file(),
        "{UNTRACKED_SOURCE_INVALID}：{source_file:?}"
    );
    let destination_file = destination.join(relative);
    ensure!(
        !destination_file.is_symlink()
            && !destination_file
                .try_exists()
                .with_context(|| format!("{UNTRACKED_PATH_FAILED}：{destination_file:?}"))?,
        "{UNTRACKED_DESTINATION_EXISTS}：{destination_file:?}"
    );
    Ok(())
}

fn validate_parent_directories(root: &Path, relative: &Path, require_exists: bool) -> Result<()> {
    let mut parent = root.to_path_buf();
    for component in relative
        .parent()
        .context(UNTRACKED_PATH_FAILED)?
        .components()
    {
        parent.push(component);
        ensure!(
            !parent.is_symlink(),
            "{UNTRACKED_PARENT_INVALID}：{parent:?}"
        );
        let exists = parent
            .try_exists()
            .with_context(|| format!("{UNTRACKED_PATH_FAILED}：{parent:?}"))?;
        ensure!(
            (exists && parent.is_dir()) || (!exists && !require_exists),
            "{UNTRACKED_PARENT_INVALID}：{parent:?}"
        );
    }
    Ok(())
}

fn copy_untracked_files(
    source: &Path,
    destination: &Path,
    files: &[PathBuf],
    verbose: bool,
) -> Result<()> {
    for relative in files {
        // 套用 patch 後再次檢查路徑，再以 create_new 防止覆寫稍早檢查後出現的項目。
        validate_untracked_file(source, destination, relative)?;
        let source_file = source.join(relative);
        let destination_file = destination.join(relative);
        std::fs::create_dir_all(destination_file.parent().context(UNTRACKED_PATH_FAILED)?)
            .with_context(|| format!("{UNTRACKED_COPY_FAILED}：{destination_file:?}"))?;
        validate_parent_directories(destination, relative, true)?;
        let mut input = File::open(&source_file)
            .with_context(|| format!("{UNTRACKED_COPY_FAILED}：{source_file:?}"))?;
        let permissions = input
            .metadata()
            .with_context(|| format!("{UNTRACKED_PATH_FAILED}：{source_file:?}"))?
            .permissions();
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination_file)
            .with_context(|| format!("{UNTRACKED_COPY_FAILED}：{destination_file:?}"))?;
        std::io::copy(&mut input, &mut output)
            .with_context(|| format!("{UNTRACKED_COPY_FAILED}：{destination_file:?}"))?;
        output
            .set_permissions(permissions)
            .with_context(|| format!("{UNTRACKED_COPY_FAILED}：{destination_file:?}"))?;
        if verbose {
            eprintln!("[DEBUG] 已複製未追蹤檔案：{relative:?}");
        }
    }
    Ok(())
}
