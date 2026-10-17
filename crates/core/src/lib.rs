//! The rules Spielstand never breaks, as pure functions shared by server and agent.

pub mod decide;
pub mod pack;
pub mod plan;
pub mod portable;
pub mod retention;

pub use spielstand_protocol as protocol;

/// Version ids sort by creation time, so file listings and logs read in order.
pub fn new_version_id(now: chrono::DateTime<chrono::Utc>) -> String {
    let short = uuid::Uuid::new_v4().simple().to_string();
    format!("{}-{}", now.format("%Y%m%dT%H%M%S%3fZ"), &short[..12])
}

/// A file-system-safe name for a game title, used in mirror paths.
pub fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("Baldur's Gate 3"), "baldur-s-gate-3");
        assert_eq!(slug("  Hades II  "), "hades-ii");
    }

    #[test]
    fn ids_sort_by_time() {
        let a = new_version_id(chrono::Utc::now());
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_version_id(chrono::Utc::now());
        assert!(a < b);
    }
}
