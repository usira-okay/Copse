use std::cell::RefCell;
use std::cmp::Reverse;
use std::io::IsTerminal;

use anyhow::{Result, ensure};
use inquire::Select;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Matcher, Utf32Str};

const TERMINAL_REQUIRED: &str =
    "模糊選單需要互動式終端，請將標準錯誤連接至終端或使用完整命令參數。";
const SELECT_HELP: &str = "輸入文字搜尋，↑/↓ 選擇，Enter 確認，Esc 取消";

/// 顯示可取消的模糊選單，回傳選項在原始清單中的索引。
pub(crate) fn fuzzy_select(prompt: &str, items: &[impl AsRef<str>]) -> Result<Option<usize>> {
    ensure!(std::io::stderr().is_terminal(), TERMINAL_REQUIRED);
    // 共用匹配器與 Unicode 緩衝區，避免每個候選項目重複配置匹配記憶體。
    let matching = RefCell::new((Matcher::default(), Vec::new()));
    let scorer = |query: &str, _option: &&str, label: &str, _index: usize| {
        let (matcher, buffer) = &mut *matching.borrow_mut();
        score(query, label, matcher, buffer)
    };
    let selection = Select::new(prompt, items.iter().map(AsRef::as_ref).collect())
        .with_starting_cursor(0)
        .with_scorer(&scorer)
        .with_sorter(&sort_options)
        .with_help_message(SELECT_HELP)
        .raw_prompt_skippable()?;
    Ok(selection.map(|selected| selected.index))
}

fn score(query: &str, label: &str, matcher: &mut Matcher, buffer: &mut Vec<char>) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let pattern = Atom::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
        false,
    );
    pattern
        .score(Utf32Str::new(label, buffer), matcher)
        .map(i64::from)
}

fn sort_options(options: &mut [(usize, i64)]) {
    options.sort_unstable_by_key(|&(index, score)| (Reverse(score), index));
}

#[cfg(test)]
mod tests {
    use super::{score, sort_options};
    use nucleo_matcher::Matcher;

    fn ranked_indices(query: &str, labels: &[&str]) -> Vec<usize> {
        let mut matcher = Matcher::default();
        let mut buffer = Vec::new();
        let mut options: Vec<_> = labels
            .iter()
            .enumerate()
            .filter_map(|(index, label)| {
                score(query, label, &mut matcher, &mut buffer).map(|score| (index, score))
            })
            .collect();
        sort_options(&mut options);
        options.into_iter().map(|(index, _)| index).collect()
    }

    #[test]
    fn matches_non_contiguous_chinese_text() {
        let labels = [
            "📂  直接從來源建立 worktree",
            "🌿  從來源建立新分支與 worktree",
        ];

        assert_eq!(ranked_indices("建分", &labels), vec![1]);
    }

    #[test]
    fn matches_non_contiguous_branch_names() {
        let labels = ["main", "feature/test-branch", "release"];

        assert_eq!(ranked_indices("ftbr", &labels), vec![1]);
    }

    #[test]
    fn empty_query_preserves_original_order_and_safe_first_option() {
        let labels = [
            "一般刪除",
            "強制刪除",
            "一般刪除並清理分支",
            "強制刪除並清理分支",
        ];

        assert_eq!(ranked_indices("", &labels), vec![0, 1, 2, 3]);
    }

    #[test]
    fn equal_scores_use_original_indices() {
        let mut options = [(3, 5), (1, 5), (0, 3), (2, 5)];

        sort_options(&mut options);

        assert_eq!(options, [(1, 5), (2, 5), (3, 5), (0, 3)]);
    }

    #[test]
    fn duplicate_labels_keep_distinct_original_indices() {
        let labels = ["main", "worktree", "worktree"];

        assert_eq!(ranked_indices("wkt", &labels), vec![1, 2]);
    }

    #[test]
    fn query_punctuation_is_literal() {
        let labels = ["important", "feature/!important"];

        assert_eq!(ranked_indices("!important", &labels), vec![1]);
    }
}
