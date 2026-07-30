use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use regex::{Captures, Regex};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ProfileFile {
    #[serde(default = "schema_version")]
    schema_version: u32,
    name: String,
    pattern: String,
    #[serde(default)]
    timestamp_format: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
}

fn schema_version() -> u32 {
    1
}

#[derive(Debug)]
pub struct LogProfile {
    name: String,
    pattern: Regex,
    timestamp_format: Option<String>,
    timezone: Option<FixedOffset>,
}

impl LogProfile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read log profile {}", path.display()))?;
        let file: ProfileFile = serde_json::from_str(&content)
            .with_context(|| format!("invalid log profile {}", path.display()))?;
        if file.schema_version != 1 {
            bail!(
                "unsupported log profile schema {}, expected 1",
                file.schema_version
            );
        }
        if file.name.trim().is_empty() {
            bail!("log profile name cannot be empty");
        }

        let pattern = Regex::new(&file.pattern)
            .with_context(|| format!("invalid pattern in {}", path.display()))?;
        for required in ["level", "message"] {
            if !pattern
                .capture_names()
                .flatten()
                .any(|name| name == required)
            {
                bail!("log profile pattern must define a `{required}` capture");
            }
        }
        let timezone = file
            .timezone
            .as_deref()
            .map(parse_offset)
            .transpose()
            .with_context(|| format!("invalid timezone in {}", path.display()))?;

        Ok(Self {
            name: file.name,
            pattern,
            timestamp_format: file.timestamp_format,
            timezone,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn captures<'a>(&self, line: &'a str) -> Option<Captures<'a>> {
        self.pattern.captures(line)
    }

    pub fn timestamp(&self, value: &str) -> Option<DateTime<Utc>> {
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
            return Some(timestamp.with_timezone(&Utc));
        }

        let format = self.timestamp_format.as_deref()?;
        if let Ok(timestamp) = DateTime::parse_from_str(value, format) {
            return Some(timestamp.with_timezone(&Utc));
        }

        let local = NaiveDateTime::parse_from_str(value, format).ok()?;
        self.timezone?
            .from_local_datetime(&local)
            .single()
            .map(|timestamp| timestamp.with_timezone(&Utc))
    }
}

fn parse_offset(value: &str) -> Result<FixedOffset> {
    let compact = value.replace(':', "");
    if compact.len() != 5 || !matches!(compact.as_bytes()[0], b'+' | b'-') {
        bail!("timezone must use +HH:MM or -HH:MM");
    }
    let hours: i32 = compact[1..3].parse()?;
    let minutes: i32 = compact[3..5].parse()?;
    if hours > 23 || minutes > 59 {
        bail!("timezone is out of range");
    }
    let seconds = hours * 3600 + minutes * 60;
    if compact.starts_with('-') {
        FixedOffset::west_opt(seconds).context("timezone is out of range")
    } else {
        FixedOffset::east_opt(seconds).context("timezone is out of range")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_offset;

    #[test]
    fn parses_timezone_offsets() {
        assert_eq!(parse_offset("+08:00").unwrap().local_minus_utc(), 28_800);
        assert_eq!(parse_offset("-0530").unwrap().local_minus_utc(), -19_800);
        assert!(parse_offset("UTC").is_err());
    }
}
