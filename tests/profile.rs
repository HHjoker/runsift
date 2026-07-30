use std::fs;

use runsift::profile::LogProfile;

mod support;

#[test]
fn parses_negative_timezone_offset() {
    let directory = support::temp_directory("timezone");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("profile.json");
    fs::write(
        &path,
        r#"{
  "name": "timezone-test",
  "pattern": "^(?P<level>[^|]+)\\|(?P<message>.*)$",
  "timestamp_format": "%Y-%m-%d %H:%M:%S",
  "timezone": "-05:30"
}"#,
    )
    .unwrap();
    let profile = LogProfile::load(&path).unwrap();
    let timestamp = profile.timestamp("2026-07-30 10:00:00").unwrap();
    let _ = fs::remove_dir_all(directory);

    assert_eq!(timestamp.to_rfc3339(), "2026-07-30T15:30:00+00:00");
}

#[test]
fn rejects_invalid_timezone() {
    let directory = support::temp_directory("invalid-timezone");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("profile.json");
    fs::write(
        &path,
        r#"{
  "name": "timezone-test",
  "pattern": "^(?P<level>[^|]+)\\|(?P<message>.*)$",
  "timezone": "UTC"
}"#,
    )
    .unwrap();
    let result = LogProfile::load(&path);
    let _ = fs::remove_dir_all(directory);

    assert!(result.is_err());
}
