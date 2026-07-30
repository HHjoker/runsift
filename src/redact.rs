use std::sync::LazyLock;

use regex::{Captures, Regex};

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}").unwrap());
static SECRET_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(api[_-]?key|access[_-]?token|auth[_-]?token|password|passwd|secret)\b(\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;]+)"#,
    )
    .unwrap()
});
static JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b").unwrap()
});

pub fn text(input: &str, enabled: bool) -> String {
    if !enabled {
        return input.to_owned();
    }

    let value = BEARER.replace_all(input, "${1}<redacted>");
    let value = JWT.replace_all(&value, "<redacted-jwt>");
    SECRET_FIELD
        .replace_all(&value, |captures: &Captures<'_>| {
            format!("{}{}<redacted>", &captures[1], &captures[2])
        })
        .into_owned()
}
