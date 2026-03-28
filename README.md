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
# Binary is at target/release/copse
```

## Usage

Run `copse` inside any git repository:

```bash
copse
```

You will be presented with an interactive menu to select an action:

```
Select an action:
> create
  delete
  move-to
  copy-changes
```

### Options

| Flag | Short | Description |
|------|-------|-------------|
| `--verbose` | `-v` | Show verbose debug output |
| `--absolute-path` | `-a` | Use absolute paths when creating worktrees (default: relative) |

```bash
copse --verbose
copse -a
```

### Actions

#### create

1. Select a branch or tag (with fuzzy search)
   - Sorted by: local branches → remote branches → local tags → remote tags
2. Choose to create the worktree directly, or create a new branch first
3. The worktree is created at `../<repoName>-worktree/<branch_name>`
   - Example: repo at `~/ProjectA` with branch `feature-1` → `~/ProjectA-worktree/feature-1`
4. Optionally move to the new worktree after creation

#### delete

1. Multi-select worktrees to delete (Space to toggle, Enter to confirm)
2. Choose a deletion mode:
   - Delete worktree but keep branch
   - Force delete worktree but keep branch
   - Delete worktree and delete branch
   - Force delete worktree and delete branch
3. Warnings are shown for worktrees with uncommitted changes
4. Confirmation is required before proceeding

#### move-to

1. Select a worktree from the list
2. Confirm the move
3. Outputs a `cd` command (and `COPSE_CD:<path>` for shell integration)

#### copy-changes

1. Select a source worktree to copy changes from
2. Changes are applied as a diff to the current worktree
   - If the diff applies cleanly, it is applied directly
   - If there are conflicts, a 3-way merge is attempted with conflict markers
3. Optionally copy untracked files as well

### Shell integration

`copse` cannot change the shell's working directory directly. The `move-to` and `create` actions output a `COPSE_CD:<path>` line that a shell wrapper can use.

Example wrapper for Bash/Zsh:

```bash
copse_wrapper() {
  local output
  output=$(copse "$@")
  echo "$output"
  local cd_line
  cd_line=$(echo "$output" | grep '^COPSE_CD:' | tail -1)
  if [ -n "$cd_line" ]; then
    cd "${cd_line#COPSE_CD:}" || return
  fi
}
alias cps='copse_wrapper'
```

## Requirements

- Git ≥ 2.15 (for `git worktree` support)
- Rust ≥ 1.85 (edition 2024)

## License

Licensed under the [Apache License 2.0](LICENSE).
