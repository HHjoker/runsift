use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::cli::{InspectArgs, InspectQuery};
use crate::model::{Event, Manifest};

pub fn run(args: InspectArgs) -> Result<i32> {
    let manifest: Manifest = serde_json::from_reader(open(&args.bundle, "manifest.json")?)?;
    ensure!(
        matches!(manifest.schema_version, 2 | 3),
        "unsupported evidence schema {}",
        manifest.schema_version
    );

    let result = match args.query {
        InspectQuery::Overview => json!({ "manifest": manifest }),
        InspectQuery::Search { text, limit } => search(&args.bundle, &text, limit as usize)?,
        InspectQuery::Evidence { event_id, around } => {
            evidence(&args.bundle, &event_id, around as usize)?
        }
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &result)?;
    writeln!(stdout)?;
    Ok(0)
}

fn open(bundle: &Path, name: &str) -> Result<BufReader<File>> {
    let path = bundle.join(name);
    File::open(&path)
        .map(BufReader::new)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn events(bundle: &Path) -> Result<impl Iterator<Item = Result<Event>>> {
    Ok(open(bundle, "events.jsonl")?
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(line) if line.trim().is_empty() => None,
            line => Some((|| {
                Ok(serde_json::from_str(&line?)
                    .with_context(|| format!("invalid events.jsonl line {}", index + 1))?)
            })()),
        }))
}

fn search(bundle: &Path, text: &str, limit: usize) -> Result<Value> {
    ensure!(!text.trim().is_empty(), "search text cannot be empty");
    let mut matches = Vec::new();
    for event in events(bundle)? {
        let event = event?;
        if event.message.contains(text) {
            if matches.len() == limit {
                return Ok(json!({ "events": matches, "truncated": true }));
            }
            matches.push(event);
        }
    }
    Ok(json!({ "events": matches, "truncated": false }))
}

fn evidence(bundle: &Path, event_id: &str, around: usize) -> Result<Value> {
    let mut events = events(bundle)?;
    let target = events
        .find(|event| {
            event.as_ref().is_err() || event.as_ref().is_ok_and(|e| e.event_id == event_id)
        })
        .transpose()?
        .with_context(|| format!("event {event_id} was not found"))?;

    let same_artifact = |event: &Event| event.evidence.artifact == target.evidence.artifact;
    let mut before = VecDeque::with_capacity(around);
    let mut after = Vec::new();
    if around > 0 {
        for event in self::events(bundle)? {
            let event = event?;
            if event.event_id == event_id {
                break;
            }
            if same_artifact(&event) {
                if before.len() == around {
                    before.pop_front();
                }
                before.push_back(event);
            }
        }
        for event in events {
            let event = event?;
            if same_artifact(&event) {
                after.push(event);
                if after.len() == around {
                    break;
                }
            }
        }
    }
    Ok(json!({
        "event": target,
        "before": before,
        "after": after,
        "representation": "parsed_event",
        "offset_basis": "original_source"
    }))
}
