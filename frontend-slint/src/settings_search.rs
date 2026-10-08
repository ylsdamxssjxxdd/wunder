//! §9.1 settings search: one place that decides which of the twelve categories
//! a query keeps.
//!
//! The match runs natively because Slint 1.18 strings expose only is-empty /
//! to-lowercase / starts-with / ends-with / replace-all — there is no substring
//! test, and no bitwise operator to ship a mask either. The result is therefore
//! one boolean per category, in navigation order.
//!
//! The native shell and the in-memory preview both call this, so the two can
//! never disagree about what the search box does.

/// Category keywords for the search box. i18n lives on the Slint side, so the
/// filter keys off the same two languages the shell ships.
const SETTINGS_CATEGORY_LABELS: [[&str; 3]; 12] = [
    ["常规", "常规设置", "general"],
    ["模型设置", "模型配置", "models"],
    ["工具管理", "工具", "tools"],
    ["智能体设置", "智能体", "agent"],
    ["桌宠", "形象库", "companion"],
    ["定时任务", "计划任务", "scheduled"],
    ["记忆碎片", "记忆", "memories"],
    ["渠道设置", "渠道", "channels"],
    ["运行记录", "用量统计", "activity"],
    ["提示词包", "提示词", "prompts"],
    ["归档线程", "归档", "archived"],
    ["帮助", "关于", "help"],
];

/// How many categories §9.2 defines. The shell's initial flag list has to match.
pub const SETTINGS_CATEGORY_COUNT: usize = SETTINGS_CATEGORY_LABELS.len();

/// One flag per category: an empty query keeps everything, otherwise a category
/// matches when any of its keywords contains the needle.
pub fn category_flags(query: &str) -> Vec<bool> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return vec![true; SETTINGS_CATEGORY_COUNT];
    }
    SETTINGS_CATEGORY_LABELS
        .iter()
        .map(|keywords| {
            keywords
                .iter()
                .any(|keyword| keyword.to_lowercase().contains(&needle))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{category_flags, SETTINGS_CATEGORY_COUNT};

    #[test]
    fn empty_query_keeps_every_category() {
        let flags = category_flags("   ");
        assert_eq!(flags.len(), SETTINGS_CATEGORY_COUNT);
        assert!(flags.iter().all(|flag| *flag));
    }

    #[test]
    fn query_matches_case_insensitively_across_languages() {
        let models = category_flags("MODEL");
        assert_eq!(models.iter().filter(|flag| **flag).count(), 1);
        assert!(models[1], "models should match its english keyword");
        let chinese = category_flags("记忆");
        assert!(chinese[6], "memories should match its chinese label");
        assert!(!chinese[0]);
    }

    /// A query that matches nothing must yield no flags: the navigation then
    /// shows its empty state rather than silently listing everything.
    #[test]
    fn unknown_query_hides_every_category() {
        let flags = category_flags("zzzz-no-such-category");
        assert_eq!(flags.iter().filter(|flag| **flag).count(), 0);
    }

    /// The shell hard-codes the initial flag list, so the count is a contract.
    #[test]
    fn category_count_matches_the_navigation() {
        assert_eq!(SETTINGS_CATEGORY_COUNT, 12);
    }
}
