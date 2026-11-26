//! Read the server URL out of the runtime's stdout banner.
//!
//! `dsh web` prints exactly one line carrying its token URL; everything else on
//! stdout is ordinary log traffic that must not be mistaken for it. The tagged
//! line (`dsh web: …token=…`) wins; if no line is tagged, the first http(s) URL
//! is used as a fallback. Either way the parsed origin must be a loopback host,
//! since the shell always launches `dsh` with `--host 127.0.0.1`.

use tauri::Url;

/// Whether `host` is a loopback address the shell's WebView may open.
///
/// Trim surrounding whitespace, strip IPv6 brackets, then compare
/// case-insensitively against the shell's loopback set.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    let normalized = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    matches!(normalized.as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// Pull the URL out of a runtime output line.
pub fn web_url(line: &str) -> Option<String> {
    let url = tagged_url(line).or_else(|| loose_url(line))?;
    let parsed = url.parse::<Url>().ok()?;
    let http_scheme = matches!(parsed.scheme(), "http" | "https");
    let loopback_host = parsed.host_str().is_some_and(is_loopback_host);
    (http_scheme && loopback_host).then_some(url)
}

fn tagged_url(line: &str) -> Option<String> {
    if !line.to_ascii_lowercase().contains("dsh web") {
        return None;
    }
    let url = loose_url(line)?;
    url.contains("token=").then_some(url)
}

fn loose_url(line: &str) -> Option<String> {
    let start = match (line.find("http://"), line.find("https://")) {
        (Some(http), Some(https)) => http.min(https),
        (Some(http), None) => http,
        (None, Some(https)) => https,
        (None, None) => return None,
    };
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = &rest[..end];
    url.parse::<Url>().ok().map(|_| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{tagged_url, web_url};

    #[test]
    fn reads_the_url_from_the_runtime_banner() {
        let line = "dsh web: http://127.0.0.1:44123/?token=abc-DEF_123";
        assert_eq!(web_url(line).as_deref(), Some("http://127.0.0.1:44123/?token=abc-DEF_123"));
    }

    #[test]
    fn a_tagged_line_needs_a_token() {
        assert_eq!(tagged_url("dsh web: http://127.0.0.1:44123/"), None);
        assert_eq!(tagged_url("loading plugins: http://127.0.0.1:1/?token=x"), None);
        assert_eq!(tagged_url("dsh web: http://127.0.0.1:1/?token=x").as_deref(), Some("http://127.0.0.1:1/?token=x"));
    }

    #[test]
    fn ignores_lines_without_a_url() {
        assert_eq!(web_url("[dsh] loading plugins"), None);
        assert_eq!(web_url("http://"), None);
    }

    #[test]
    fn falls_back_to_a_loose_url_when_nothing_is_tagged() {
        assert_eq!(web_url("serving at http://127.0.0.1:9/").as_deref(), Some("http://127.0.0.1:9/"));
    }

    #[test]
    fn picks_the_earliest_url_when_https_precedes_http() {
        let line = "serving at https://127.0.0.1:1234/ (health http://127.0.0.1:1234/health)";
        assert_eq!(web_url(line).as_deref(), Some("https://127.0.0.1:1234/"));
    }

    #[test]
    fn rejects_an_external_url_from_a_tagged_line() {
        assert_eq!(
            web_url("dsh web: https://evil.example/x?token=abc"),
            None
        );
    }

    #[test]
    fn rejects_an_external_url_from_an_untagged_line() {
        assert_eq!(
            web_url("loaded plugin from https://evil.example/x"),
            None
        );
    }

    #[test]
    fn accepts_localhost_as_a_loopback_host() {
        assert_eq!(
            web_url("dsh web: http://localhost:44123/?token=abc").as_deref(),
            Some("http://localhost:44123/?token=abc")
        );
    }

    #[test]
    fn accepts_bracketed_ipv6_loopback() {
        assert_eq!(
            web_url("dsh web: http://[::1]:44123/?token=abc").as_deref(),
            Some("http://[::1]:44123/?token=abc")
        );
    }

    #[test]
    fn accepts_https_on_a_loopback_host() {
        assert_eq!(
            web_url("dsh web: https://127.0.0.1:44123/?token=abc").as_deref(),
            Some("https://127.0.0.1:44123/?token=abc")
        );
    }

    #[test]
    fn rejects_other_non_loopback_hosts() {
        assert_eq!(web_url("serving at http://0.0.0.0:9/"), None);
        assert_eq!(web_url("serving at http://example.com:9/"), None);
    }

    #[test]
    fn accepts_an_uppercase_localhost_host() {
        assert_eq!(
            web_url("serving at http://LOCALHOST:9/").as_deref(),
            Some("http://LOCALHOST:9/")
        );
    }

    #[test]
    fn rejects_unsupported_schemes_even_on_loopback() {
        assert_eq!(web_url("serving at ftp://127.0.0.1:9/"), None);
    }
}
