# 發布指南（Release Guide）

本文件說明如何將 `git_worktree_copse` 發布到 [crates.io](https://crates.io/crates/git_worktree_copse)，包含一次性的環境設定，以及每次發布新版本時需要執行的步驟。

發布流程由 [`.github/workflows/publish-crate.yml`](../.github/workflows/publish-crate.yml) 自動化：推送符合 `vX.Y.Z` 格式的 git tag 後，會自動執行版本檢查、測試、建置與 `cargo publish --dry-run`；通過後，需要有人到 GitHub 網頁上手動核准，才會真正執行 `cargo publish`。

## 一、前置設定（一次性，需要 repo admin 權限）

> ⚠️ 以下步驟需要 `usira-okay/Copse` repo 的 admin 權限才能操作。如果你目前只有 read 權限，請將這份文件轉交給有權限的人完成設定。

1. **取得 crates.io API Token**
   - 使用擁有此 crate 發布權限的 crates.io 帳號登入 <https://crates.io/settings/tokens>
   - 建立一組新的 API Token，範圍（scope）只勾選 **`publish-update`** 即可（這個 crate 已經存在於 crates.io，CI 只需要能發布新版本，不需要 `publish-new` 或 `yank`）
   - 複製產生的 token（僅會顯示一次，請妥善保存）

2. **建立 GitHub Environment**
   - 到 GitHub repo 頁面 → **Settings → Environments → New environment**
   - 名稱輸入 `crates-io`（必須與 workflow 檔案中的 `environment: crates-io` 完全一致）
   - ⚠️ Environment 剛建立時**預設沒有任何保護規則**，任何 job 都可以立刻無阻礙地使用它

3. **設定 Required reviewers（務必在新增 secret 之前完成這一步）**
   - 同樣在 `crates-io` Environment 頁面 → **Deployment protection rules → Required reviewers**
   - 勾選並加入至少一位（通常是你自己或其他 maintainer）作為核准者
   - 儲存後，**重新整理頁面確認** Required reviewers 確實顯示為已啟用
   - 這一步是關鍵的安全機制：之後每次 tag 觸發 workflow，`publish` job 都會停在這裡等待核准，才會真正呼叫 `cargo publish`

4. **確認核准機制已生效後，才設定 Environment secret**
   - ⚠️ **順序很重要**：一定要先完成步驟 3 並確認 Required reviewers 已啟用，才能進行這一步。如果先加入 secret、後設定 Required reviewers，中間會有一段空窗期——這段期間只要有人推送 tag，`publish` job 會在沒有任何人核准的情況下直接執行，等同於完全繞過本文件設計的人工核准保護
   - 在 `crates-io` Environment 頁面 → **Environment secrets → Add secret**
   - Name 填入 `CARGO_REGISTRY_TOKEN`
   - Value 貼上步驟 1 取得的 crates.io token

完成以上設定後，之後每次發布都不需要重複這些步驟。

## 二、每次發布步驟

1. 確認 `main` 分支已經包含這次要發布的所有變更（若在其他 worktree 或分支工作，記得先 `git checkout main && git pull`，確保本地 `main` 是最新的）
2. 修改 `Cargo.toml` 的 `version` 欄位（例如從 `0.1.4` 改成 `0.1.5`）
3. 更新 `Cargo.lock`（**不可省略**：`Cargo.lock` 內也記錄了本套件自己的版本號，若沒有同步更新，`verify` job 裡所有帶 `--locked` 的指令都會直接失敗）：
   ```bash
   cargo check
   ```
4. Commit 並 push 到 `main`：
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "chore: bump version to 0.1.5"
   git push origin main
   ```
5. 建立並推送對應的 tag（**tag 版本必須與 Cargo.toml 完全一致**，否則 workflow 會直接失敗）：
   ```bash
   git tag v0.1.5
   git push origin v0.1.5
   ```
6. 到 GitHub repo 的 **Actions** 分頁，確認 `Publish to crates.io` workflow 已被觸發，並等待 `verify` job 執行完成（會跑版本檢查、`cargo test`、`cargo build --release`、`cargo publish --dry-run`）
7. `verify` 通過後，`publish` job 會顯示為等待核准。到該 job 頁面點選 **Review deployments**，勾選 `crates-io` 後按下 **Approve and deploy**
   - ⚠️ 注意：`cargo publish --dry-run`（第 6 步）**不會因為「這個版本號已經發布過」而失敗**——它會在執行紀錄裡印出 `already exists on crates.io index` 警告，但仍以成功（exit code 0）結束。也就是說，就算 `verify` 完全通過，`publish` job 仍有可能在核准後才真正失敗（crates.io 拒絕重複版本號）。核准前建議先手動到 crates.io 頁面確認這個版本號還沒發布過，也可以到 `verify` job 的 dry-run 步驟日誌搜尋 `already exists on crates.io index` 字樣作為額外訊號
8. 等待 `publish` job 執行完成，到 <https://crates.io/crates/git_worktree_copse> 確認新版本已經上架

## 三、疑難排解

### Tag 版本與 Cargo.toml 版本不一致

`verify` job 會顯示類似以下錯誤：

```
Tag 版本 (v0.1.5) 與 Cargo.toml 版本 (0.1.4) 不一致，請確認後重新推送 tag
```

處理方式：先刪除錯誤的 tag，修正版本後重新建立、推送。

```bash
# 刪除本地 tag
git tag -d v0.1.5
# 刪除遠端 tag
git push origin --delete v0.1.5
```

修正 `Cargo.toml` 或重新確認版本號後，重新執行「二、每次發布步驟」中的第 5 步。

### crates.io 回傳 401 / 403（token 過期或權限不足）

代表 `CARGO_REGISTRY_TOKEN` 已失效，或該帳號沒有這個 crate 的發布權限。處理方式：

1. 回到 <https://crates.io/settings/tokens> 重新產生一組新的 token
2. 到 GitHub repo 的 `crates-io` Environment，更新 `CARGO_REGISTRY_TOKEN` secret 的內容
3. 重新核准該次 `publish` job（或重新推送 tag 觸發一次新的 workflow）

### 版本號重複，crates.io 拒絕發布

crates.io 不允許覆蓋已經發布過的版本號（即使該版本已經被 yank）。這是 crates.io 的原生限制，workflow 不會、也無法繞過。

**注意**：`verify` job 的 `cargo publish --dry-run` **不會因此失敗**——重複版本號只會讓 dry-run 在執行紀錄印出 `already exists on crates.io index` 警告，但仍以成功（exit code 0）結束。真正的拒絕只會發生在 `publish` job（也就是核准之後）。如果核准後才發現失敗，屬於預期行為，不是 workflow 壞了。處理方式：將 `Cargo.toml` 改成下一個尚未使用過的版本號，重新走一次「二、每次發布步驟」。

### 需要撤下已發布的錯誤版本

crates.io 不能刪除已發布的版本，但可以「yank」，讓它不會被新專案當作預設可安裝的版本（已經依賴該版本的專案仍可繼續使用該版本）：

```bash
cargo login   # 如果本機尚未登入，貼上你的 crates.io token
cargo yank --version 0.1.5
```

如果誤 yank，也可以復原：

```bash
cargo yank --version 0.1.5 --undo
```

## 四、安全設計說明

- **為什麼 `CARGO_REGISTRY_TOKEN` 放在 Environment secret，而不是一般的 repository secret？**
  一般 repository secret，repo 內任何 workflow 的任何 job 都能讀取。放在 `crates-io` Environment 的 secret，只有明確設定 `environment: crates-io` 的 job 才能存取，而這個 job 又受到 Required reviewers 保護，等於多一層存取控制。`verify` job 完全不需要、也讀不到這個 secret。

- **為什麼 `publish` job 需要人工核准？**
  `cargo publish` 是不可逆的操作：版本號一旦發布就無法覆蓋，即使 yank 也無法讓已下載的使用者收回。加上人工核准，可以在真正發布前，讓人多一次確認「這個版本、這次推送的 tag，真的是我要發布的」，避免例如 tag 打錯分支、CI 腳本誤觸發等情況直接造成無法收回的發布。
