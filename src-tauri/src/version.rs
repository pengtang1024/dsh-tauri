//! Compare `@deepseek-ai/dsh` versions the way npm/semver does.
//!
//! Only what the updater needs: numeric core segments plus an optional
//! prerelease (`0.2.2-rc.5`); build metadata is ignored. Prerelease precedence
//! follows semver 2.0 §11:
//!
//! * a release outranks its own prereleases — `0.2.2` > `0.2.2-rc.5`;
//! * identifiers are compared field by field (`rc.5` vs `rc.4`);
//! * a numeric identifier compares numerically (`rc.10` > `rc.9`) and always
//!   ranks below an alphanumeric one (`beta` < `rc`);
//! * text identifiers compare by ASCII (`alpha` < `beta` < `rc`);
//! * a shorter identifier list loses when it is a prefix of a longer one
//!   (`rc` < `rc.1`).

use std::cmp::Ordering;

/// `true` when `upstream` is strictly newer than `local`.
pub fn is_newer(upstream: &str, local: &str) -> bool {
    compare(upstream, local) == Ordering::Greater
}

/// Compare two versions: `Greater` means "update to the left-hand side".
pub fn compare(upstream: &str, local: &str) -> Ordering {
    let (up_core, up_pre) = split(upstream);
    let (lo_core, lo_pre) = split(local);
    let core = compare_core(&up_core, &lo_core);
    if core != Ordering::Equal {
        return core;
    }
    match (up_pre, lo_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(up), Some(lo)) => compare_pre(&up, &lo)
    }
}

/// `(numeric core segments, prerelease)`; malformed segments read as 0.
fn split(raw: &str) -> (Vec<u64>, Option<String>) {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let without_build = trimmed.split('+').next().unwrap_or(trimmed);
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease.to_owned())),
        None => (without_build, None)
    };
    let segments = core.split('.').map(|part| part.parse::<u64>().unwrap_or(0)).collect();
    (segments, prerelease)
}

fn compare_core(up: &[u64], lo: &[u64]) -> Ordering {
    for index in 0..up.len().max(lo.len()) {
        let left = up.get(index).copied().unwrap_or(0);
        let right = lo.get(index).copied().unwrap_or(0);
        if left != right {
            return left.cmp(&right);
        }
    }
    Ordering::Equal
}

fn compare_pre(up: &str, lo: &str) -> Ordering {
    let up: Vec<&str> = up.split('.').collect();
    let lo: Vec<&str> = lo.split('.').collect();
    for (left, right) in up.iter().zip(lo.iter()) {
        let order = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right)
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    up.len().cmp(&lo.len())
}

#[cfg(test)]
mod tests {
    use super::{compare, is_newer};
    use std::cmp::Ordering;

    #[test]
    fn identical_versions_are_not_newer() {
        assert!(!is_newer("0.2.2-rc.5", "0.2.2-rc.5"));
        assert_eq!(compare("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn a_bigger_numeric_segment_is_newer() {
        assert!(is_newer("0.2.3", "0.2.2"));
        assert!(is_newer("0.3.0", "0.2.99"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.2", "0.2.3"));
        assert!(is_newer("0.10.0", "0.9.0"), "0.10 must beat 0.9 numerically");
    }

    #[test]
    fn missing_segments_count_as_zero() {
        assert_eq!(compare("1.0", "1.0.0"), Ordering::Equal);
        assert!(is_newer("1.0.1", "1.0"));
    }

    #[test]
    fn a_release_outranks_its_own_prerelease() {
        assert!(is_newer("0.2.2", "0.2.2-rc.5"));
        assert!(is_newer("0.2.3-rc.1", "0.2.2"));
        assert!(!is_newer("0.2.2-rc.5", "0.2.2"));
    }

    #[test]
    fn prerelease_numbering_is_numeric_not_text() {
        assert!(is_newer("0.2.2-rc.10", "0.2.2-rc.9"));
        assert!(is_newer("0.2.2-rc.5", "0.2.2-rc.4"));
        assert!(!is_newer("0.2.2-rc.5", "0.2.2-rc.5"));
    }

    #[test]
    fn prerelease_labels_compare_by_ascii() {
        assert!(is_newer("0.2.2-rc.1", "0.2.2-beta.9"));
        assert!(!is_newer("0.2.2-beta.1", "0.2.2-rc.1"));
        // A numeric identifier ranks below an alphanumeric one (semver §11).
        assert!(is_newer("0.2.2-alpha", "0.2.2-1"));
    }

    #[test]
    fn a_shorter_prerelease_loses_when_it_is_a_prefix() {
        assert!(is_newer("0.2.2-rc.1", "0.2.2-rc"));
        assert!(!is_newer("0.2.2-rc", "0.2.2-rc.1"));
    }

    #[test]
    fn build_metadata_and_a_leading_v_are_ignored() {
        assert_eq!(compare("0.2.2+build.7", "0.2.2"), Ordering::Equal);
        assert_eq!(compare("v0.2.2", "0.2.2"), Ordering::Equal);
    }
}
