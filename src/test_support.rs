use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SANDBOX: AtomicU64 = AtomicU64::new(0);

/// 以絕對路徑隔離真實 Git 測試，避免修改程序的工作目錄。
pub struct TestSandbox {
    pub root: PathBuf,
}

impl TestSandbox {
    /// 建立不與其他平行測試共用的暫存目錄。
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "copse-batch-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("hooks")).unwrap();
        Self {
            root: crate::git::strip_unc_prefix_path(std::fs::canonicalize(root).unwrap()),
        }
    }

    /// 建立具有初始提交的 repo，並關閉測試不需要的簽章與 hook。
    pub fn repo(&self, name: &str) -> PathBuf {
        let repo = self.root.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "--initial-branch=main"]);
        self.git(&repo, &["config", "user.name", "Copse Test"]);
        self.git(&repo, &["config", "user.email", "copse@example.invalid"]);
        self.git(&repo, &["config", "commit.gpgsign", "false"]);
        self.git(
            &repo,
            &[
                "config",
                "core.hooksPath",
                self.root.join("hooks").to_str().unwrap(),
            ],
        );
        self.git(&repo, &["commit", "--allow-empty", "-m", "initial"]);
        repo
    }

    /// 在 sandbox 內的相對位置建立指定分支的 linked worktree。
    pub fn worktree(&self, repo: &Path, relative_path: &str, branch: &str) -> PathBuf {
        let path = self.root.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        self.git(
            repo,
            &["worktree", "add", "-b", branch, path.to_str().unwrap()],
        );
        path
    }

    /// 執行並檢查 fixture 命令，失敗時保留診斷輸出。
    pub fn git(&self, repo: &Path, args: &[&str]) -> Output {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git {args:?} 失敗：{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("無法清除測試目錄 {:?}：{error}", self.root);
        }
    }
}
