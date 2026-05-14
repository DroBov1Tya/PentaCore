use regex::Regex;
use std::sync::LazyLock;

static LINK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)href=["'](/[^"']+|https?://[^"']+)["']"#).unwrap());
static API_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)["'](/api/[^"']+)["']"#).unwrap());
static JWT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"ey[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"#).unwrap()
});

pub fn analyze_response(body: &str) -> Option<String> {
    let mut hints = Vec::new();

    let mut links: Vec<String> = LINK_REGEX
        .captures_iter(body)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    links.sort();
    links.dedup();
    if !links.is_empty() {
        hints.push(format!(
            "Found {} links (e.g. {})",
            links.len(),
            links.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        ));
    }

    let mut apis: Vec<String> = API_REGEX
        .captures_iter(body)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    apis.sort();
    apis.dedup();
    if !apis.is_empty() {
        hints.push(format!(
            "Found {} potential API endpoints (e.g. {})",
            apis.len(),
            apis.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        ));
    }

    let jwts: Vec<String> = JWT_REGEX
        .captures_iter(body)
        .filter_map(|cap| cap.get(0).map(|m| m.as_str().to_string()))
        .collect();
    if !jwts.is_empty() {
        hints.push(format!(
            "CRITICAL: Found {} potential JWT tokens in response!",
            jwts.len()
        ));
    }

    if hints.is_empty() {
        None
    } else {
        Some(format!(
            "\n\n[PASSIVE RECON HINT]\n- {}",
            hints.join("\n- ")
        ))
    }
}
