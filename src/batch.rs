use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::Event;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    counts: String,
    completion: String,
    stages: Vec<String>,
}

pub(crate) fn check(
    events: impl Iterator<Item = Result<Event>>,
    batch_id: &str,
    path: &Path,
) -> Result<Value> {
    let profile: Profile = serde_json::from_reader(BufReader::new(File::open(path)?))
        .with_context(|| format!("invalid batch profile {}", path.display()))?;
    ensure!(
        profile.stages.len() >= 2
            && profile.stages.iter().enumerate().all(|(i, stage)| {
                !stage.trim().is_empty() && !profile.stages[..i].contains(stage)
            }),
        "profile requires at least two distinct, nonempty stages"
    );
    let counts = compile(&profile.counts, &["batch", "stage", "count"])?;
    let completion = compile(&profile.completion, &["batch"])?;
    let mut completed = None;
    let mut observations = vec![None; profile.stages.len()];
    let mut conflicts = vec![None; profile.stages.len()];
    for event in events {
        let event = event?;
        for fields in completion.captures_iter(&event.message) {
            if fields.name("batch").is_some_and(|m| m.as_str() == batch_id) {
                completed.get_or_insert_with(|| event.event_id.clone());
            }
        }
        for fields in counts.captures_iter(&event.message) {
            if fields.name("batch").is_none_or(|m| m.as_str() != batch_id) {
                continue;
            }
            let Some(stage) = fields.name("stage") else {
                continue;
            };
            let Some(i) = profile.stages.iter().position(|s| s == stage.as_str()) else {
                continue;
            };
            let count = fields
                .name("count")
                .and_then(|m| m.as_str().parse::<u64>().ok())
                .with_context(|| format!("invalid count at event {}", event.event_id))?;
            match &observations[i] {
                Some((previous, _)) if *previous != count => {
                    conflicts[i].get_or_insert_with(|| (count, event.event_id.clone()));
                }
                None => observations[i] = Some((count, event.event_id.clone())),
                _ => {}
            }
        }
    }
    let stages: Vec<_> = profile
        .stages
        .iter()
        .zip(&observations)
        .zip(&conflicts)
        .map(|((stage, observed), conflict)| {
            json!({
                "stage": stage,
                "count": observed.as_ref().map(|(count, _)| count),
                "event_id": observed.as_ref().map(|(_, id)| id),
                "conflicting_observation": conflict.as_ref().map(|(count, id)| json!({
                    "count": count, "event_id": id
                }))
            })
        })
        .collect();
    let mut first_difference = None;
    if completed.is_some() {
        for (i, pair) in observations.windows(2).enumerate() {
            match pair {
                [Some((left, _)), Some((right, _))]
                    if conflicts[i].is_none() && conflicts[i + 1].is_none() =>
                {
                    if left != right {
                        first_difference = Some(json!({
                            "from_stage": profile.stages[i],
                            "to_stage": profile.stages[i + 1],
                            "expected": left,
                            "observed": right,
                            "direction": if right < left { "shortfall" } else { "excess" },
                            "difference": left.abs_diff(*right)
                        }));
                        break;
                    }
                }
                _ => break,
            }
        }
    }
    let missing_stages: Vec<_> = profile
        .stages
        .iter()
        .zip(&observations)
        .filter_map(|(stage, observed)| observed.is_none().then_some(stage))
        .collect();
    let status = if first_difference.is_some() {
        "difference"
    } else if completed.is_none()
        || !missing_stages.is_empty()
        || conflicts.iter().any(Option::is_some)
    {
        "insufficient_evidence"
    } else {
        "balanced"
    };
    Ok(json!({
        "batch_id": batch_id,
        "status": status,
        "completion_event_id": completed,
        "missing_stages": missing_stages,
        "stages": stages,
        "first_difference": first_difference
    }))
}

fn compile(pattern: &str, required: &[&str]) -> Result<Regex> {
    let regex = Regex::new(pattern)?;
    for name in required {
        ensure!(
            regex.capture_names().flatten().any(|group| group == *name),
            "batch profile regex requires capture {name}"
        );
    }
    Ok(regex)
}
