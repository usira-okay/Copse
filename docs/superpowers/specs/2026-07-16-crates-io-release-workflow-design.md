# 設計文件：crates.io 自動發布 Workflow

- 日期：2026-07-16
- 專案：Copse（crate 名稱 `git_worktree_copse`）
- 狀態：待實作

## 背景

`git_worktree_copse` 目前已手動發布至 crates.io（最新版本 0.1.4），版本號更新方式是直接修改 `Cargo.toml` 後 commit，再由開發者手動於本機執行 `cargo publish`。目前 repo 內沒有任何 `.github/workflows`、沒有 git tag、也沒有 GitHub Release。

本次目標：新增一份 GitHub Actions workflow，讓「推送版本 tag」可以自動完成發布前的品質把關，並在人工核准後將該版本發布到 crates.io；同時提供一份 Markdown 文件，說明從一次性環境設定到每次發布所需的所有手動步驟。

## 範圍

**In scope**
- 新增 `.github/workflows/publish-crate.yml`
- 新增 `docs/RELEASING.md`（正體中文）
- tag 版本與 `Cargo.toml` 版本一致性檢查
- 發布前品質把關：`cargo test`、`cargo build --release`、`cargo publish --dry-run`
- 以 GitHub Environment 核准機制保護實際 `cargo publish` 步驟

**Out of scope（本次不做，未來可視需求擴充）**
- 自動建立 GitHub Release / Release Notes
- 自動 bump 版本號或自動建立 tag
- `cargo clippy` / `cargo fmt --check` 品質關卡
- 跨平台（macOS/Windows）建置或發布 binary
- CHANGELOG 自動產生

## 架構設計

### 觸發條件

```yaml
on:
  push:
    tags:
      - 'v*.*.*'
```

推送符合語意化版本的 tag（例如 `v0.1.5`）即觸發。Workflow 層級套用最小權限 `permissions: contents: read`，並加上 `concurrency` group（以 `github.ref` 為 key）避免同一 tag 被重複推送時互相干擾。

### Job 1：`verify`(自動執行，不需核准，也不接觸 publish token)

1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable` 安裝 stable Rust
3. `Swatinem/rust-cache@v2` 快取 cargo registry / target，加速重複建置
4. **版本一致性檢查**：以 `GITHUB_REF_NAME` 取得 tag（去除開頭 `v`），並用 `cargo metadata --no-deps --format-version=1` 讀出 `Cargo.toml` 內的實際 version，兩者不一致則印出明確錯誤訊息並以非 0 結束碼中止，避免發錯版本
5. `cargo test --locked`
6. `cargo build --release --locked`
7. `cargo publish --dry-run --locked`

> 關鍵設計點：`cargo publish --dry-run` 不需要 crates.io 認證即可執行（已透過網路搜尋確認），因此 `verify` job 完全不需要、也不會拿到 `CARGO_REGISTRY_TOKEN`。這個 job 在 tag 一推送就會自動執行，不需人工介入。

### Job 2：`publish`（`needs: verify`，綁定 GitHub Environment，需人工核准）

1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable`
3. `cargo publish --locked`，透過 `CARGO_REGISTRY_TOKEN` 環境變數帶入密鑰

此 job 設定 `environment: crates-io`。`CARGO_REGISTRY_TOKEN` 存放在該 Environment 的 secret（而非一般 repository secret），只有通過 Environment 核准流程的 job 才能存取，形成清楚的安全邊界：**未經核准，任何自動觸發的流程都拿不到可以實際發布的憑證**。Environment 需設定 Required reviewers，讓每次真正發布前都要有人手動按下核准。

### 錯誤處理原則

- 任何一步（版本檢查、test、build、dry-run）失敗，整個 workflow 在 `verify` 階段就停止，`publish` job 因 `needs: verify` 不會被觸發，crates.io 不會受到任何影響
- 版本不一致時的錯誤訊息需同時列出 tag 版本與 Cargo.toml 版本，方便直接判斷是哪邊打錯
- crates.io 本身拒絕重複版本號發布（不可覆蓋已發布版本），此為 crates.io 原生行為，workflow 不需要另外處理，但會在 `docs/RELEASING.md` 的疑難排解章節說明

## docs/RELEASING.md 大綱

僅使用正體中文撰寫，涵蓋：

1. **前置設定（一次性，需 repo admin 權限）**
   - 建立/取得 crates.io API Token（需為該 crate 的 owner）
   - 於 GitHub repo Settings → Environments 建立名為 `crates-io` 的 Environment
   - 於該 Environment 新增 secret `CARGO_REGISTRY_TOKEN`
   - 於該 Environment 設定 Required reviewers
   - 明確註記：目前操作者僅有此 repo 的 READ 權限，以上設定需請具備 admin 權限者協助完成
2. **每次發布步驟 checklist**
   - 確認 main 分支已包含要發布的所有變更
   - 修改 `Cargo.toml` 的 `version`
   - commit + push 到 main
   - 建立並推送 tag `vX.Y.Z`（須與 Cargo.toml 版本一致）
   - 至 GitHub Actions 頁面確認 `verify` job 通過
   - 至待核准的 Environment 核准 `publish` job
   - 至 crates.io 確認新版本已上架
3. **疑難排解**
   - tag 與 Cargo.toml 版本不一致
   - token 過期或 401/403
   - 重複版本號被 crates.io 拒絕
   - 如何刪除本地與遠端錯誤的 tag（`git tag -d` / `git push --delete origin`）
   - 如何以 `cargo yank --version X.Y.Z` 撤下已發布但有問題的版本
4. **安全設計說明**
   - 為何 token 存放在 Environment secret 而非 repository secret
   - 為何 `publish` job 需要人工核准

## 待辦與限制

- 目前操作帳號對 `usira-okay/Copse` 僅有 READ 權限，無法透過程式建立 GitHub Environment、新增 secret 或設定 Required reviewers；這些步驟已完整記錄於 `docs/RELEASING.md`，需由具備權限者手動完成
- 本次實作僅新增 workflow 檔案與文件，不會建立任何 tag 或觸發實際發布
