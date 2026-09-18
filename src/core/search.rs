//! Fuzzy search over non-secret item metadata.

use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};

use crate::model::{ItemKey, ItemSummary};

pub const DEFAULT_MAX_RESULTS: usize = 50;

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    /// Index into the item list the index was built from.
    pub item: usize,
    pub score: u32,
    /// Matched character positions in the title, sorted and unique.
    pub title_indices: Vec<u32>,
}

struct Entry {
    title: Utf32String,
    /// Username, email, URL hosts, and vault name.
    others: Vec<Utf32String>,
    sort_title: String,
}

pub struct SearchIndex {
    entries: Vec<Entry>,
    matcher: Matcher,
}

impl std::fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SearchIndex({} entries)", self.entries.len())
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::build(&[])
    }
}

impl SearchIndex {
    pub fn build(items: &[ItemSummary]) -> Self {
        let entries = items
            .iter()
            .map(|item| {
                let mut others: Vec<Utf32String> = Vec::new();
                others.extend(item.username.as_deref().map(Utf32String::from));
                others.extend(item.email.as_deref().map(Utf32String::from));
                others.extend(
                    item.urls
                        .iter()
                        .filter_map(|u| url_host(u))
                        .map(Utf32String::from),
                );
                others.push(Utf32String::from(item.vault_name.as_str()));
                Entry {
                    title: Utf32String::from(item.title.as_str()),
                    others,
                    sort_title: item.display_title().to_lowercase(),
                }
            })
            .collect();
        let mut config = Config::DEFAULT;
        config.prefer_prefix = true;
        Self {
            entries,
            matcher: Matcher::new(config),
        }
    }

    /// Returns up to `max` rows. An empty query lists items by recency, then by title.
    pub fn search(
        &mut self,
        query: &str,
        recency: impl Fn(&ItemKey) -> Option<i64>,
        items: &[ItemSummary],
        max: usize,
    ) -> Vec<ResultRow> {
        let pattern = Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut rows: Vec<ResultRow> = if pattern.atoms.is_empty() {
            (0..self.entries.len())
                .map(|item| ResultRow {
                    item,
                    score: 0,
                    title_indices: Vec::new(),
                })
                .collect()
        } else {
            let mut rows = Vec::new();
            for (item, entry) in self.entries.iter().enumerate() {
                if let Some(row) = score_entry(&mut self.matcher, &pattern.atoms, entry, item) {
                    rows.push(row);
                }
            }
            rows
        };

        let entries = &self.entries;
        let recency_of = |row: &ResultRow| items.get(row.item).and_then(|i| recency(&i.key));
        rows.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| recency_of(b).cmp(&recency_of(a)))
                .then_with(|| entries[a.item].sort_title.cmp(&entries[b.item].sort_title))
                .then_with(|| a.item.cmp(&b.item))
        });
        rows.truncate(max);
        rows
    }
}

/// Title matches count double.
const TITLE_WEIGHT: u32 = 2;

/// Every atom must match at least one field; each atom contributes its best field score.
fn score_entry(
    matcher: &mut Matcher,
    atoms: &[Atom],
    entry: &Entry,
    item: usize,
) -> Option<ResultRow> {
    let mut total = 0;
    let mut title_indices = Vec::new();
    for atom in atoms {
        let mut indices = Vec::new();
        let title = atom
            .indices(entry.title.slice(..), matcher, &mut indices)
            .map(|s| u32::from(s) * TITLE_WEIGHT);
        let other = entry
            .others
            .iter()
            .filter_map(|h| atom.score(h.slice(..), matcher))
            .max()
            .map(u32::from);
        let best = title.max(other)?;
        if title.is_some() {
            title_indices.extend(indices);
        }
        total += best;
    }
    title_indices.sort_unstable();
    title_indices.dedup();
    Some(ResultRow {
        item,
        score: total,
        title_indices,
    })
}

/// `https://www.example.com/path` → `www.example.com`.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemKind;

    fn item(
        id: &str,
        title: &str,
        user: Option<&str>,
        url: Option<&str>,
        vault: &str,
    ) -> ItemSummary {
        ItemSummary {
            key: ItemKey::new("s", id),
            vault_name: vault.into(),
            kind: ItemKind::Login,
            title: title.into(),
            username: user.map(Into::into),
            email: None,
            subtitle: user.map(Into::into),
            urls: url.map(|u| vec![u.to_owned()]).unwrap_or_default(),
            totp_fields: vec![],
            fields: vec![],
            modified_at: 0,
        }
    }

    fn corpus() -> Vec<ItemSummary> {
        vec![
            item(
                "gh",
                "GitHub",
                Some("octocat"),
                Some("https://github.com/login"),
                "Personal",
            ),
            item(
                "gh-work",
                "GitHub",
                Some("work-octo"),
                Some("https://github.com/"),
                "Work",
            ),
            item(
                "mail",
                "Example Mail",
                Some("gitlover"),
                Some("https://mail.example.org"),
                "Personal",
            ),
            item(
                "bank",
                "Bank",
                None,
                Some("https://www.bank.example/login"),
                "Finance",
            ),
            item("zeta", "zeta", None, None, "Personal"),
            item("alpha", "Alpha", None, None, "Personal"),
        ]
    }

    fn ids(items: &[ItemSummary], rows: &[ResultRow]) -> Vec<String> {
        rows.iter()
            .map(|r| items[r.item].key.item.0.clone())
            .collect()
    }

    fn run(query: &str) -> Vec<String> {
        let items = corpus();
        let mut index = SearchIndex::build(&items);
        let rows = index.search(query, |_| None, &items, DEFAULT_MAX_RESULTS);
        ids(&items, &rows)
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(run("GITHUB")[..2], ["gh", "gh-work"]);
    }

    #[test]
    fn out_of_order_fragments() {
        let hits = run("hub git");
        assert!(hits.contains(&"gh".to_owned()));
    }

    #[test]
    fn title_match_outranks_username_match() {
        let hits = run("git");
        let mail_pos = hits.iter().position(|h| h == "mail").unwrap();
        let gh_pos = hits.iter().position(|h| h == "gh").unwrap();
        assert!(gh_pos < mail_pos, "{hits:?}");
    }

    #[test]
    fn url_host_and_vault_are_searchable() {
        assert_eq!(run("bank.example"), ["bank"]);
        assert_eq!(run("finance"), ["bank"]);
        assert_eq!(run("github work"), ["gh-work"]);
    }

    #[test]
    fn url_path_is_not_searchable() {
        assert!(!run("login").contains(&"bank".to_owned()));
    }

    #[test]
    fn same_title_in_two_vaults_both_appear() {
        let hits = run("github");
        assert!(hits.contains(&"gh".to_owned()) && hits.contains(&"gh-work".to_owned()));
    }

    #[test]
    fn no_match_is_empty() {
        assert!(run("qqqxxx").is_empty());
    }

    #[test]
    fn results_are_capped() {
        let items: Vec<_> = (0..120)
            .map(|i| item(&i.to_string(), &format!("site {i}"), None, None, "V"))
            .collect();
        let mut index = SearchIndex::build(&items);
        assert_eq!(index.search("site", |_| None, &items, 50).len(), 50);
        assert_eq!(index.search("", |_| None, &items, 10).len(), 10);
    }

    #[test]
    fn empty_query_orders_by_recency_then_title() {
        let items = corpus();
        let mut index = SearchIndex::build(&items);
        let recency = |k: &ItemKey| match k.item.0.as_str() {
            "bank" => Some(200),
            "zeta" => Some(100),
            _ => None,
        };
        let rows = index.search("", recency, &items, 50);
        assert_eq!(
            ids(&items, &rows),
            ["bank", "zeta", "alpha", "mail", "gh", "gh-work"]
        );
    }

    #[test]
    fn recency_breaks_score_ties() {
        let items = corpus();
        let mut index = SearchIndex::build(&items);
        let recency = |k: &ItemKey| (k.item.0 == "gh-work").then_some(5);
        let rows = index.search("github", recency, &items, 50);
        assert_eq!(ids(&items, &rows)[0], "gh-work");
    }

    #[test]
    fn title_indices_mark_matched_characters() {
        let items = corpus();
        let mut index = SearchIndex::build(&items);
        let rows = index.search("bnk", |_| None, &items, 50);
        assert_eq!(rows[0].title_indices, vec![0, 2, 3]);
    }

    #[test]
    fn url_hosts() {
        assert_eq!(url_host("https://a.example/x?y"), Some("a.example"));
        assert_eq!(url_host("user@b.example"), Some("b.example"));
        assert_eq!(url_host("c.example"), Some("c.example"));
        assert_eq!(url_host("https://"), None);
    }

    #[test]
    fn empty_index() {
        let mut index = SearchIndex::default();
        assert!(index.search("x", |_| None, &[], 50).is_empty());
    }
}
