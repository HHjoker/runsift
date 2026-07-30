use std::fs;

use runsift::model::{TestFramework, TestStatus};
use runsift::test_report;

mod support;

#[test]
fn parses_googletest_junit_xml() {
    let directory = support::temp_directory("gtest");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("gtest.xml");
    fs::write(
        &path,
        r#"<?xml version="1.0"?>
<testsuites tests="2" failures="1" name="AllTests">
  <testsuite name="ParserTest" tests="2">
    <testcase name="AcceptsValid" status="run" result="completed" time="0.005"/>
    <testcase name="RejectsBad" status="run" result="completed" time="0.010">
      <failure message="password=hunter2">expected true</failure>
    </testcase>
  </testsuite>
</testsuites>"#,
    )
    .unwrap();

    let imported = test_report::import(&path, "tests/000-gtest.xml".to_owned(), true).unwrap();
    let _ = fs::remove_dir_all(directory);
    assert_eq!(imported.report.framework, TestFramework::GoogleTest);
    assert_eq!(imported.report.total, 2);
    assert_eq!(imported.report.failed, 1);
    assert_eq!(imported.report.tests[1].status, TestStatus::Failed);
    assert_eq!(
        imported.report.tests[1].message.as_deref(),
        Some("password=<redacted>")
    );
    assert!(imported.content.contains("password=&lt;redacted&gt;"));
}

#[test]
fn parses_ctest_junit_xml() {
    let directory = support::temp_directory("ctest");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("ctest.xml");
    fs::write(
        &path,
        r#"<testsuite name="CTest" tests="1" failures="0">
  <testcase name="Parser.AcceptsValid" classname="Parser" time="0.025"/>
</testsuite>"#,
    )
    .unwrap();

    let report = test_report::import(&path, "tests/000-ctest.xml".to_owned(), true)
        .unwrap()
        .report;
    let _ = fs::remove_dir_all(directory);
    assert_eq!(report.framework, TestFramework::CTest);
    assert_eq!(report.passed, 1);
    assert_eq!(report.tests[0].duration_ms, Some(25.0));
}
