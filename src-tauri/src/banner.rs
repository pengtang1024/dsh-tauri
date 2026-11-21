//! Read the server URL out of the runtime's stdout banner.
//!
//! `dsh web` prints exactly one line carrying its token URL; everything else on
//! stdout is ordinary log traffic that must not be mistaken for it. The tagged
//! line (`dsh web: …token=…`) wins; if no line is tagged, any http(s) URL is
//! accepted so a banner change upstream cannot leave the shell without a window.

use tauri::Url;

/// Pull the URL out of a runtime output line.
pub fn web_url(line: &str) -> Option<String> {
    tagged_url(line).or_else(|| loose_url(line))
}

fn tagged_url(line: &str) -> Option<String> {
    if !line.to_ascii_lowercase().contains("dsh web") {
        return None;
    }
    let url = loose_url(line)?;
    url.contains("token=").then_some(url)
}

fn loose_url(line: &str) -> Option<String> {
    let start = line.find("http://").or_else(|| line.find("https://"))?;
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
}
