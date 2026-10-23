//! Picks the verdict for a URL from a list of rules.

use crate::config::Mode;
use crate::rule::{Rule, Verdict};
use crate::url::ParsedUrl;

/// The verdict for `url`: the most specific matching rule wins, and the stricter rule breaks a tie.
///
/// If no rule matches, or `url` has no host, the mode decides (allow for a blocklist, block for an
/// allowlist). The result doesn't depend on the order of `rules`.
pub fn evaluate(mode: Mode, rules: &[Rule], url: &str) -> Verdict {
    let Some(url) = ParsedUrl::parse(url) else {
        return mode.default_verdict();
    };
    rules
        .iter()
        .filter(|rule| rule.pattern.matches(&url))
        .max_by_key(|rule| (rule.pattern.specificity(), rule.verdict))
        .map_or_else(|| mode.default_verdict(), |rule| rule.verdict)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{RuleAction, UrlPattern};

    const HIDDEN: Verdict = Verdict::Restrict(RuleAction::HideFeed);
    const BLOCKED: Verdict = Verdict::Restrict(RuleAction::Block);

    fn rule(pattern: &str, verdict: Verdict) -> Rule {
        Rule {
            pattern: UrlPattern::parse(pattern).unwrap(),
            verdict,
        }
    }

    /// The `coding` configuration from `docs/DESIGN.md`, with the `social` list included.
    fn coding_rules() -> Vec<Rule> {
        vec![
            rule("x.com", BLOCKED),
            rule("instagram.com", BLOCKED),
            rule("tiktok.com", BLOCKED),
            rule("reddit.com", HIDDEN),
            rule("youtube.com", HIDDEN),
            rule("reddit.com/r/rust/comments/*", Verdict::Allow),
            rule("docs.rs", Verdict::Allow),
            rule("github.com", Verdict::Allow),
        ]
    }

    fn coding(url: &str) -> Verdict {
        evaluate(Mode::Blocklist, &coding_rules(), url)
    }

    #[test]
    fn design_doc_examples() {
        assert_eq!(coding("https://www.reddit.com/"), HIDDEN);
        assert_eq!(
            coding("https://www.reddit.com/r/rust/comments/1abc/borrowck_question/"),
            Verdict::Allow
        );
        assert_eq!(coding("https://x.com/home"), BLOCKED);
        assert_eq!(coding("https://docs.rs/iced"), Verdict::Allow);
        assert_eq!(coding("https://example.org"), Verdict::Allow);
    }

    #[test]
    fn unmatched_pages_follow_the_mode() {
        let rules = [rule("docs.google.com", Verdict::Allow)];
        assert_eq!(
            evaluate(Mode::Allowlist, &rules, "https://docs.google.com/d/1"),
            Verdict::Allow
        );
        assert_eq!(
            evaluate(Mode::Allowlist, &rules, "https://news.ycombinator.com"),
            BLOCKED
        );
        assert_eq!(evaluate(Mode::Allowlist, &rules, "not a url"), BLOCKED);
        assert_eq!(evaluate(Mode::Blocklist, &[], "not a url"), Verdict::Allow);
    }

    #[test]
    fn stricter_rule_wins_a_tie() {
        let rules = [
            rule("a.com/x/*", BLOCKED),
            rule("a.com/x/*", Verdict::Allow),
        ];
        assert_eq!(evaluate(Mode::Blocklist, &rules, "a.com/x/1"), BLOCKED);
    }

    fn reddit_prefix() -> impl Strategy<Value = String> {
        (
            prop::sample::select(vec!["", "http://", "https://", "HTTPS://"]),
            prop::sample::select(vec!["", "www.", "old.", "new."]),
        )
            .prop_map(|(scheme, sub)| format!("{scheme}{sub}reddit.com"))
    }

    fn query() -> impl Strategy<Value = String> {
        prop::option::of("[a-z0-9=&_]{0,20}")
            .prop_map(|q| q.map(|q| format!("?{q}")).unwrap_or_default())
    }

    proptest! {
        #[test]
        fn rust_posts_are_allowed(
            prefix in reddit_prefix(), id in "[a-z0-9]{1,8}", slug in "[a-z0-9_]{0,30}",
            slash in prop::bool::ANY, query in query(),
        ) {
            let slash = if slash { "/" } else { "" };
            let url = format!("{prefix}/r/rust/comments/{id}/{slug}{slash}{query}");
            prop_assert_eq!(coding(&url), Verdict::Allow);
        }

        #[test]
        fn reddit_front_page_is_hidden(prefix in reddit_prefix(), query in query()) {
            let with_slash = format!("{prefix}/{query}");
            let without_slash = format!("{prefix}{query}");
            prop_assert_eq!(coding(&with_slash), HIDDEN);
            prop_assert_eq!(coding(&without_slash), HIDDEN);
        }

        #[test]
        fn other_subreddits_are_hidden(prefix in reddit_prefix(), sub in "[a-z0-9_]{1,20}", rest in "[a-z0-9/]{0,20}") {
            prop_assume!(sub != "rust");
            let url = format!("{prefix}/r/{sub}/comments/{rest}");
            prop_assert_eq!(coding(&url), HIDDEN);
        }

        #[test]
        fn rule_order_does_not_matter(
            rules in Just(coding_rules()).prop_shuffle(),
            url in prop::sample::select(vec![
                "reddit.com/", "reddit.com/r/rust/comments/a/b", "x.com/home",
                "docs.rs/serde", "youtube.com/watch", "example.org",
            ]),
        ) {
            prop_assert_eq!(evaluate(Mode::Blocklist, &rules, url), coding(url));
        }
    }
}
