//! Which versions may go to the trash. Never: pinned ones, heads, anything a device holds,
//! the newest of each device, or anything younger than a week.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, Utc};

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub game: String,
    pub device: String,
    pub created_at: DateTime<Utc>,
    /// Pinned, a head, or the base of some device.
    pub protected: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Everything younger is kept.
    pub keep_all_days: i64,
    /// Up to this age one version per day is kept, older one per week.
    pub daily_days: i64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            keep_all_days: 7,
            daily_days: 90,
        }
    }
}

fn bucket(item: &Item, now: DateTime<Utc>, policy: Policy) -> Option<(String, i32, u32)> {
    let age = now - item.created_at;
    if age < Duration::days(policy.keep_all_days) {
        return None;
    }
    let d = item.created_at.date_naive();
    if age < Duration::days(policy.daily_days) {
        Some((item.game.clone(), d.year(), d.ordinal()))
    } else {
        let week = d.iso_week();
        Some((item.game.clone(), week.year(), 1000 + week.week()))
    }
}

/// The ids that may move to the trash now.
pub fn expired(items: &[Item], now: DateTime<Utc>, policy: Policy) -> Vec<String> {
    let mut keep: BTreeSet<&str> = items
        .iter()
        .filter(|i| i.protected)
        .map(|i| i.id.as_str())
        .collect();

    let mut newest_per_device: BTreeMap<(&str, &str), &Item> = BTreeMap::new();
    for item in items {
        let key = (item.game.as_str(), item.device.as_str());
        if newest_per_device
            .get(&key)
            .is_none_or(|n| n.created_at < item.created_at)
        {
            newest_per_device.insert(key, item);
        }
    }
    keep.extend(newest_per_device.values().map(|i| i.id.as_str()));

    let mut newest_per_bucket: BTreeMap<(String, i32, u32), &Item> = BTreeMap::new();
    for item in items {
        match bucket(item, now, policy) {
            None => {
                keep.insert(item.id.as_str());
            }
            Some(b) => {
                if newest_per_bucket
                    .get(&b)
                    .is_none_or(|n| n.created_at < item.created_at)
                {
                    newest_per_bucket.insert(b, item);
                }
            }
        }
    }
    keep.extend(newest_per_bucket.values().map(|i| i.id.as_str()));

    items
        .iter()
        .filter(|i| !keep.contains(i.id.as_str()))
        .map(|i| i.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, device: &str, days_ago: i64, hours: i64, protected: bool, now: DateTime<Utc>) -> Item {
        Item {
            id: id.into(),
            game: "G".into(),
            device: device.into(),
            created_at: now - Duration::days(days_ago) - Duration::hours(hours),
            protected,
        }
    }

    #[test]
    fn keeps_recent_protected_and_one_per_day() {
        let now = Utc::now();
        let items = vec![
            item("recent", "a", 1, 0, false, now),
            item("day10-early", "a", 10, 5, false, now),
            item("day10-late", "a", 10, 1, false, now),
            item("old-pinned", "a", 200, 0, true, now),
            item("old-b", "b", 300, 0, false, now),
        ];
        let gone = expired(&items, now, Policy::default());
        assert_eq!(gone, vec!["day10-early".to_string()]);
    }

    proptest::proptest! {
        #[test]
        fn never_expires_protected_or_newest(ages in proptest::collection::vec((0i64..400, 0i64..24, proptest::bool::ANY, 0u8..3), 1..40)) {
            let now = Utc::now();
            let items: Vec<Item> = ages.iter().enumerate().map(|(n, (d, h, p, dev))| {
                item(&format!("v{n}"), &format!("d{dev}"), *d, *h, *p, now)
            }).collect();
            let gone: BTreeSet<String> = expired(&items, now, Policy::default()).into_iter().collect();
            for i in &items {
                if i.protected { proptest::prop_assert!(!gone.contains(&i.id)); }
            }
            for dev in 0..3u8 {
                let newest = items.iter().filter(|i| i.device == format!("d{dev}")).max_by_key(|i| i.created_at);
                if let Some(n) = newest { proptest::prop_assert!(!gone.contains(&n.id)); }
            }
        }
    }
}
