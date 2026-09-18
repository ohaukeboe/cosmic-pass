//! Recently used item records.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ItemKey, ItemSummary};

pub const MAX_RECORDS: usize = 200;

/// When and how often an item was copied. Holds no values (FR-026).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub key: ItemKey,
    pub last_used: i64,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageTable {
    records: HashMap<ItemKey, UsageRecord>,
}

impl UsageTable {
    pub fn from_records(records: Vec<UsageRecord>) -> Self {
        let mut table = Self {
            records: records.into_iter().map(|r| (r.key.clone(), r)).collect(),
        };
        table.evict();
        table
    }

    /// Records sorted by most recent use first.
    pub fn records(&self) -> Vec<UsageRecord> {
        let mut records: Vec<_> = self.records.values().cloned().collect();
        records.sort_by(|a, b| {
            b.last_used
                .cmp(&a.last_used)
                .then_with(|| a.key.cmp(&b.key))
        });
        records
    }

    pub fn record(&mut self, key: &ItemKey, now: i64) {
        let entry = self
            .records
            .entry(key.clone())
            .or_insert_with(|| UsageRecord {
                key: key.clone(),
                last_used: now,
                count: 0,
            });
        entry.last_used = now;
        entry.count = entry.count.saturating_add(1);
        self.evict();
    }

    pub fn recency(&self, key: &ItemKey) -> Option<i64> {
        self.records.get(key).map(|r| r.last_used)
    }

    /// Drops records whose item no longer exists.
    pub fn prune(&mut self, items: &[ItemSummary]) {
        let live: std::collections::HashSet<&ItemKey> = items.iter().map(|i| &i.key).collect();
        self.records.retain(|k, _| live.contains(k));
    }

    fn evict(&mut self) {
        while self.records.len() > MAX_RECORDS {
            let oldest = self
                .records
                .values()
                .min_by(|a, b| {
                    a.last_used
                        .cmp(&b.last_used)
                        .then_with(|| a.key.cmp(&b.key))
                })
                .map(|r| r.key.clone());
            match oldest {
                Some(key) => self.records.remove(&key),
                None => break,
            };
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::tests::login;

    fn key(id: &str) -> ItemKey {
        ItemKey::new("s", id)
    }

    #[test]
    fn record_sets_time_and_counts() {
        let mut t = UsageTable::default();
        assert_eq!(t.recency(&key("a")), None);
        t.record(&key("a"), 10);
        t.record(&key("a"), 20);
        assert_eq!(t.recency(&key("a")), Some(20));
        let r = t.records();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].count, 2);
    }

    #[test]
    fn count_saturates() {
        let mut t = UsageTable::from_records(vec![UsageRecord {
            key: key("a"),
            last_used: 0,
            count: u32::MAX,
        }]);
        t.record(&key("a"), 1);
        assert_eq!(t.records()[0].count, u32::MAX);
    }

    #[test]
    fn oldest_records_are_evicted() {
        let mut t = UsageTable::default();
        for i in 0..=MAX_RECORDS {
            t.record(&key(&i.to_string()), i64::try_from(i).unwrap());
        }
        assert_eq!(t.len(), MAX_RECORDS);
        assert_eq!(t.recency(&key("0")), None);
        assert!(t.recency(&key("1")).is_some());
    }

    #[test]
    fn prune_drops_missing_items() {
        let mut t = UsageTable::default();
        t.record(&key("a"), 1);
        t.record(&key("gone"), 2);
        t.prune(&[login("a", "A")]);
        assert_eq!(t.len(), 1);
        assert!(t.recency(&key("gone")).is_none());
    }

    #[test]
    fn records_round_trip_sorted_by_recency() {
        let mut t = UsageTable::default();
        t.record(&key("a"), 5);
        t.record(&key("b"), 9);
        let records = t.records();
        assert_eq!(records[0].key, key("b"));
        assert_eq!(UsageTable::from_records(records), t);
        assert!(!t.is_empty());
    }
}
