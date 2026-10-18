/// 整批共用的 worktree 與本地分支刪除策略。
#[derive(Debug, Clone, Copy)]
pub struct DeleteMode {
    pub force: bool,
    pub delete_branch: bool,
}

/// 與選單順序一致，預設保留分支且不強制刪除。
pub const MODES: [DeleteMode; 4] = [
    DeleteMode {
        force: false,
        delete_branch: false,
    },
    DeleteMode {
        force: true,
        delete_branch: false,
    },
    DeleteMode {
        force: false,
        delete_branch: true,
    },
    DeleteMode {
        force: true,
        delete_branch: true,
    },
];

/// 顯示整批刪除策略及強制清理未合併分支的影響。
pub const MODE_LABELS: [&str; 4] = [
    "刪除 worktree，保留本地分支",
    "強制刪除 worktree，保留本地分支",
    "刪除 worktree 與本地分支",
    "強制刪除 worktree 與本地分支（包含未合併分支）",
];
