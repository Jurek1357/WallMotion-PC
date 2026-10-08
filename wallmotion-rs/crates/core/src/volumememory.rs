//! Per-wallpaper volume memory: remember mute/volume for each file.
//!
//! Port of `wallmotion/volumememory.py`: pure helpers over a plain map
//! stored in the config under `volumes` (`{path: {volume, muted}}`).
//! Entries are trimmed so the config never grows without bounds.
//!
//! Note: `std` `HashMap` has no insertion order, so unlike the Python
//! dict (oldest-first eviction) the Rust trim removes an arbitrary entry
//! once over the limit. The bound is what matters; order is not relied on.

use std::collections::HashMap;

/// Maximum remembered files (mirrors Python `MAX_ENTRIES`).
pub const MAX_ENTRIES: usize = 100;
/// Fallback volume when a stored value is out of range.
pub const DEFAULT_VOLUME: u8 = 30;

/// Remembered settings for one wallpaper file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSetting {
    /// Volume 0-100.
    pub volume: u8,
    /// Muted flag.
    pub muted: bool,
}

impl VolumeSetting {
    /// Clamp a raw (volume, muted) pair into range.
    pub fn new(volume: u8, muted: bool) -> Self {
        Self {
            volume: volume.min(100),
            muted,
        }
    }
}

/// Settings remembered for `path`, or `None`. Never panics.
pub fn lookup(store: Option<&HashMap<String, VolumeSetting>>, path: &str) -> Option<VolumeSetting> {
    let store = store?;
    if path.is_empty() {
        return None;
    }
    store.get(path).copied()
}

/// Store settings for `path` as most-recent, trim to `limit` entries.
/// Empty paths are ignored. Never panics.
pub fn remember(
    store: &mut HashMap<String, VolumeSetting>,
    path: &str,
    volume: u8,
    muted: bool,
    limit: usize,
) {
    if path.is_empty() {
        return;
    }
    let limit = limit.max(1);
    store.insert(path.to_string(), VolumeSetting::new(volume, muted));
    while store.len() > limit {
        // HashMap iteration order is arbitrary; remove any single entry
        // to keep the config bounded (Python removes the oldest first —
        // same bound, different victim).
        let victim = match store.keys().next().cloned() {
            Some(k) if k != path => k,
            _ => match store.keys().nth(1).cloned() {
                Some(k) => k,
                None => break,
            },
        };
        store.remove(&victim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_missing_is_none() {
        let store: HashMap<String, VolumeSetting> = HashMap::new();
        assert_eq!(lookup(Some(&store), "a.mp4"), None);
        assert_eq!(lookup(None, "a.mp4"), None);
        let mut s = HashMap::new();
        s.insert("a.mp4".into(), VolumeSetting::new(70, false));
        assert_eq!(lookup(Some(&s), ""), None);
    }

    #[test]
    fn roundtrip() {
        let mut s = HashMap::new();
        remember(&mut s, "a.mp4", 70, false, MAX_ENTRIES);
        assert_eq!(
            lookup(Some(&s), "a.mp4"),
            Some(VolumeSetting::new(70, false))
        );
    }

    #[test]
    fn remember_clamps_volume() {
        let mut s = HashMap::new();
        remember(&mut s, "a.mp4", 255, true, MAX_ENTRIES);
        assert_eq!(lookup(Some(&s), "a.mp4").unwrap().volume, 100);
    }

    #[test]
    fn remember_trims_to_limit() {
        let mut s = HashMap::new();
        for i in 0..10 {
            remember(&mut s, &format!("f{i}.mp4"), 30, true, 5);
        }
        assert_eq!(s.len(), 5);
        // Most-recent entry always survives the trim.
        assert!(lookup(Some(&s), "f9.mp4").is_some());
    }

    #[test]
    fn empty_path_ignored() {
        let mut s = HashMap::new();
        remember(&mut s, "", 50, false, MAX_ENTRIES);
        assert!(s.is_empty());
    }
}
