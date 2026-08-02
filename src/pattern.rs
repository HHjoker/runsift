use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::logs::stable_id;
use crate::model::{Event, Pattern};

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b")
        .unwrap()
});
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b")
        .unwrap()
});
static IPV4: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap());
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b0x[0-9a-f]+\b").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d+\.\d+|\d+)\b").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());

#[derive(Debug)]
struct Accumulator {
    pattern: Pattern,
}

pub fn aggregate(events: &[Event]) -> Vec<Pattern> {
    let mut patterns = HashMap::<String, Accumulator>::new();

    for event in events {
        let template = normalize(&event.message);
        let key = format!("{:?}:{template}", event.severity);
        let pattern_id = stable_id("pat", &key);
        let event_time = event.timestamp.unwrap_or(event.observed_at);
        let entry = patterns.entry(key).or_insert_with(|| Accumulator {
            pattern: Pattern {
                pattern_id,
                severity: event.severity,
                template,
                count: 0,
                first_observed_at: event_time,
                last_observed_at: event_time,
                representative_event_ids: Vec::new(),
            },
        });

        entry.pattern.count += 1;
        entry.pattern.first_observed_at = entry.pattern.first_observed_at.min(event_time);
        entry.pattern.last_observed_at = entry.pattern.last_observed_at.max(event_time);
        if entry.pattern.representative_event_ids.len() < 3 {
            entry
                .pattern
                .representative_event_ids
                .push(event.event_id.clone());
        }
    }

    let mut result: Vec<_> = patterns.into_values().map(|entry| entry.pattern).collect();
    result.sort_by(|left, right| {
        right
            .severity
            .priority()
            .cmp(&left.severity.priority())
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.pattern_id.cmp(&right.pattern_id))
    });
    result
}

pub fn normalize(message: &str) -> String {
    let message = message.lines().next().unwrap_or(message);
    let value = TIMESTAMP.replace_all(message, "<timestamp>");
    let value = UUID.replace_all(&value, "<uuid>");
    let value = IPV4.replace_all(&value, "<ip>");
    let value = HEX.replace_all(&value, "<hex>");
    let value = NUMBER.replace_all(&value, "<num>");
    SPACES.replace_all(value.trim(), " ").into_owned()
}
