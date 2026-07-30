use runsift::redact;

#[test]
fn redacts_common_secrets() {
    let value = "Authorization: Bearer abcdefghijklmnop api_key=secret-value";
    let redacted = redact::text(value, true);
    assert_eq!(
        redacted,
        "Authorization: Bearer <redacted> api_key=<redacted>"
    );
}

#[test]
fn redaction_can_be_disabled() {
    assert_eq!(redact::text("password=hunter2", false), "password=hunter2");
}
