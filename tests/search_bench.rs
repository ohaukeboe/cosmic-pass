//! Search latency over 5,000 items (SC-002). Run with `just bench` (release build).

use std::time::{Duration, Instant};

use cosmic_pass::core::search::SearchIndex;
use cosmic_pass::core::state::{Model, Msg};
use cosmic_pass::model::{ItemKey, ItemKind};
use cosmic_pass::testing::{listing, summary};

const ITEMS: usize = 5_000;
const BUDGET: Duration = Duration::from_millis(50);

fn corpus() -> Vec<cosmic_pass::model::ItemSummary> {
    const WORDS: [&str; 12] = [
        "github", "personal", "bank", "mail", "work", "cloud", "shop", "forum", "vpn", "router",
        "school", "travel",
    ];
    (0..ITEMS)
        .map(|i| {
            let title = format!("{} {} {i}", WORDS[i % 12], WORDS[(i / 12) % 12]);
            let mut item = summary(&i.to_string(), &title, ItemKind::Login);
            item.username = Some(format!("user{i}@example.invalid"));
            item.urls = vec![format!(
                "https://{}{i}.example.invalid/login",
                WORDS[i % 12]
            )];
            item.vault_name = if i % 3 == 0 { "Work" } else { "Personal" }.into();
            item
        })
        .collect()
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() * 95 / 100]
}

#[test]
#[ignore = "timing test; run with `just bench`"]
fn search_bench() {
    let items = corpus();
    let mut index = SearchIndex::build(&items);
    let query = "github personal";
    let mut samples = Vec::new();
    for _ in 0..100 {
        for end in 1..=query.len() {
            let start = Instant::now();
            let rows = index.search(&query[..end], |_: &ItemKey| None, &items, 50);
            samples.push(start.elapsed());
            assert!(!rows.is_empty());
        }
    }
    let search_p95 = p95(samples);

    let mut model = Model::default();
    model.update(Msg::DataLoaded(listing(items)), 0);
    let mut samples = Vec::new();
    for _ in 0..100 {
        for end in 0..=query.len() {
            let start = Instant::now();
            model.update(Msg::QueryChanged(query[..end].to_owned()), 0);
            samples.push(start.elapsed());
        }
    }
    let update_p95 = p95(samples);

    println!("p95 search: {search_p95:?}, p95 update: {update_p95:?}");
    assert!(search_p95 < BUDGET, "search p95 {search_p95:?}");
    assert!(update_p95 < BUDGET, "update p95 {update_p95:?}");
}
