//! WallMotion core: platform paths, session detection, version.
//!
//! Pure logic with no GUI and no OS bindings - mirrors
//! `wallmotion/paths.py` and the session part of
//! `wallmotion/platform/linux.py` so both implementations share
//! behavior (and tests) while the Rust port grows.

pub mod autopause;
pub mod paths;
pub mod rotation;
pub mod session;

/// Crate version (matches workspace package version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        for p in parts {
            assert!(p.chars().all(|c| c.is_ascii_digit()), "{}", p);
        }
    }
}
