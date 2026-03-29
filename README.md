[English](README.md) | [繁體中文](README.zh-TW.md)

# Copse

A CLI tool to quickly control [git worktree](https://git-scm.com/docs/git-worktree) with interactive prompts and arrow-key navigation.

## Features

- **Interactive selection** — Navigate with arrow keys, fuzzy-search branches/tags, multi-select worktrees
- **create** — Create a new worktree from any local/remote branch or tag
- **delete** — Remove one or more worktrees (with optional branch cleanup)
- **move-to** — Switch to another worktree
- **copy-changes** — Copy uncommitted changes between worktrees

## Installation

### From source

```bash
cargo install --path .
```

### Build manually

```bash
git clone https://github.com/usira-okay/Copse.git
cd Copse
cargo build --release
# Binary is at target/release/git_worktree_copse
```

## Usage

Run `git_worktree_copse` inside any git repository:

```bash
git_worktree_copse
```

You will be presented with an interactive menu to select an action:

```
Select an action:
> 📂  create
  🗑️   delete
  📍  move-to
  📋  copy-changes
```

### Options

| Flag | Short | Description |
|------|-------|-------------|
| `--verbose` | `-v` | Show verbose debug output |
| `--absolute-path` | `-a` | Use absolute paths when creating worktrees (default: relative) |

```bash
git_worktree_copse --verbose
git_worktree_copse -a
```

### Actions

#### create

1. Select a branch or tag (with fuzzy search)
   - Sorted by: local branches → remote branches → local tags → remote tags
2. Choose how to create the worktree:
   - 🌿 Create a new branch from the selected ref, then create worktree (default)
   - 📂 Create worktree directly from the selected ref
3. The worktree is created at `../<repoName>-worktree/<branch_name>`
   - Example: repo at `~/ProjectA` with branch `feature-1` → `~/ProjectA-worktree/feature-1`
4. Automatically moves to the new worktree after creation

#### delete

1. Multi-select worktrees to delete (Space to toggle, Enter to confirm)
   - The main worktree and your current working directory worktree are excluded from the list
2. Choose a deletion mode:
   - 🗑️ Delete worktree(s) but keep branch(es)
   - ⚡ Force delete worktree(s) but keep branch(es)
   - 🗑️🌿 Delete worktree(s) and delete branch(es)
   - ⚡🌿 Force delete worktree(s) and delete branch(es)
3. Warnings are shown for worktrees with uncommitted changes
4. Confirmation prompt displays the selected worktrees/branches and the chosen deletion method

#### move-to

1. Select a worktree from the list
2. Immediately moves to the selected worktree (outputs `COPSE_CD:<path>` for shell integration)

#### copy-changes

1. Select a source worktree to copy changes from
2. Changes are applied as a diff to the current worktree
   - If the diff applies cleanly, it is applied directly
   - If there are conflicts, a 3-way merge is attempted with conflict markers
3. Optionally copy untracked files as well

### Shell integration

`git_worktree_copse` cannot change the shell's working directory directly. The `move-to` and `create` actions output a `COPSE_CD:<path>` line that a shell wrapper can use.

Example wrapper for Bash/Zsh:

```bash
copse_wrapper() {
  local output
  output=$(git_worktree_copse "$@")
  echo "$output"
  local cd_line
  cd_line=$(echo "$output" | grep '^COPSE_CD:' | tail -1)
  if [ -n "$cd_line" ]; then
    cd "${cd_line#COPSE_CD:}" || return
  fi
}
alias cps='copse_wrapper'
```

Example wrapper for PowerShell:

```powershell
function Invoke-Copse {
  $output = & git_worktree_copse @args
  $output | Write-Output
  $cdLine = $output | Select-String '^COPSE_CD:(.+)$' | Select-Object -Last 1
  if ($cdLine) {
    Set-Location $cdLine.Matches.Groups[1].Value
  }
}
Set-Alias -Name cps -Value Invoke-Copse
```

## Requirements

- Git ≥ 2.15 (for `git worktree` support)
- Rust ≥ 1.85 (edition 2024)

## License

Licensed under the [Apache License 2.0](LICENSE).
