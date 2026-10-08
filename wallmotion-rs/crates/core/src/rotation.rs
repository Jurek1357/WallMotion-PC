//! Wallpaper rotation: cycle through a list of files on an interval.
//!
//! Port of `wallmotion/rotation.py`: ordered file list with a cursor,
//! sequential or shuffle play order, missing files are skipped. Pure state
//! machine (only `Path::is_file` touches the fs); the UI owns the timer
//! and calls [`RotationQueue::next_file`].
//!
//! Note: the mpv sidecar loops forever (`--loop-file=inf`) and our IPC is
//! send-only, so unlike the Python backend there is no "follow video end"
//! mode — the timer always drives rotation, for images and videos alike.

use std::path::Path;

/// Offered rotation intervals in seconds (1 min … 1 h).
pub const INTERVALS: &[u64] = &[60, 300, 900, 1800, 3600];

/// Default interval (5 min), mirrors the Python UI default.
pub const DEFAULT_INTERVAL: u64 = 300;

/// Human label for an interval choice, e.g. `60 -> "1 min"`.
pub fn format_interval(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} min");
    }
    let hours = minutes as f64 / 60.0;
    let mut s = format!("{hours:.1}");
    if s.ends_with(".0") {
        s.truncate(s.len() - 2);
    }
    format!("{s} h")
}

/// Tiny xorshift64 PRNG so shuffling needs no extra dependency.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0 | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Ordered file list with a cursor. All logic, minimal I/O.
#[derive(Debug, Clone)]
pub struct RotationQueue {
    files: Vec<String>,
    index: i64,
    shuffle: bool,
    repeat: bool,
    order: Vec<usize>,
    rng: Rng,
}

impl RotationQueue {
    pub fn new(files: Vec<String>, index: i64, shuffle: bool, repeat: bool) -> Self {
        let mut q = Self {
            files,
            index: index.max(0),
            shuffle,
            repeat,
            order: vec![],
            rng: Rng(0x9e37_79b9_7f4a_7c15),
        };
        q.order = q.build_order();
        q
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn index(&self) -> i64 {
        self.index
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> bool {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: bool) {
        self.repeat = repeat;
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    fn build_order(&mut self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.files.len()).collect();
        if self.shuffle {
            // Fisher-Yates with our own PRNG.
            for i in (1..order.len()).rev() {
                let j = self.rng.below(i + 1);
                order.swap(i, j);
            }
        }
        order
    }

    fn exists(path: &str) -> bool {
        Path::new(path).is_file()
    }

    /// Append a file if it exists and is not listed. Returns added.
    pub fn add(&mut self, path: &str) -> bool {
        if path.is_empty() || !Self::exists(path) {
            return false;
        }
        if self.files.iter().any(|p| p == path) {
            return false;
        }
        self.files.push(path.to_string());
        self.order = self.build_order();
        true
    }

    /// Remove a file. Returns removed.
    pub fn remove(&mut self, path: &str) -> bool {
        let Some(pos) = self.files.iter().position(|p| p == path) else {
            return false;
        };
        self.files.remove(pos);
        self.order = self
            .order
            .iter()
            .filter_map(|&i| {
                if i == pos {
                    None
                } else if i > pos {
                    Some(i - 1)
                } else {
                    Some(i)
                }
            })
            .collect();
        if self.index >= self.files.len() as i64 {
            self.index = 0;
        }
        true
    }

    /// Switch shuffle mode and rebuild the play order from the top.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
        self.order = self.build_order();
        self.index = 0;
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.order.clear();
        self.index = 0;
    }

    /// Drop files that no longer exist. Returns removed count.
    pub fn prune_missing(&mut self) -> usize {
        let before = self.files.len();
        self.files.retain(|p| Self::exists(p));
        let removed = before - self.files.len();
        self.order = self.build_order();
        if self.index >= self.files.len() as i64 {
            self.index = 0;
        }
        removed
    }

    /// Advance the cursor and return the next existing file (or None).
    ///
    /// Without repeat the queue stops at the last item (returns None
    /// instead of wrapping) so the final wallpaper stays on.
    pub fn next_file(&mut self) -> Option<String> {
        if self.files.is_empty() || self.order.is_empty() {
            return None;
        }
        if !self.repeat && self.index >= self.order.len() as i64 - 1 {
            let last = self.files[self.order[self.order.len() - 1]].clone();
            return Self::exists(&last).then_some(last);
        }
        for _ in 0..self.order.len() {
            self.index = (self.index + 1).rem_euclid(self.order.len() as i64);
            let path = self.files[self.order[self.index as usize]].clone();
            if Self::exists(&path) {
                return Some(path);
            }
        }
        None
    }

    /// Start over from the top: `next_file()` returns the first item.
    pub fn restart(&mut self) -> Option<String> {
        self.index = -1;
        self.next_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(dir: &std::path::Path, name: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        p.to_string_lossy().into_owned()
    }

    fn fixture() -> (std::path::PathBuf, Vec<String>) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("wallmotion-rot-test-{n}"));
        let _ = std::fs::create_dir_all(&dir);
        let files = vec![
            tmp_file(&dir, "a.mp4"),
            tmp_file(&dir, "b.mp4"),
            tmp_file(&dir, "c.mp4"),
        ];
        (dir, files)
    }

    #[test]
    fn intervals_shaped() {
        assert_eq!(format_interval(30), "30 s");
        assert_eq!(format_interval(60), "1 min");
        assert_eq!(format_interval(300), "5 min");
        assert_eq!(format_interval(3600), "1 h");
        assert_eq!(format_interval(5400), "1.5 h");
    }

    #[test]
    fn sequential_cycles() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(files.clone(), 0, false, true);
        assert_eq!(q.restart().as_deref(), Some(files[0].as_str()));
        assert_eq!(q.next_file().as_deref(), Some(files[1].as_str()));
        assert_eq!(q.next_file().as_deref(), Some(files[2].as_str()));
        assert_eq!(q.next_file().as_deref(), Some(files[0].as_str()));
    }

    #[test]
    fn no_repeat_stops_at_end() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(files.clone(), 99, false, false);
        let last = q.next_file().unwrap();
        assert_eq!(last, files[files.len() - 1]);
        assert_eq!(q.next_file().as_deref(), Some(last.as_str()));
    }

    #[test]
    fn add_rejects_duplicates_and_missing() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(vec![files[0].clone()], 0, false, true);
        assert!(!q.add(&files[0]));
        assert!(!q.add("/definitely/not/here.mp4"));
        assert!(q.add(&files[1]));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn remove_keeps_order_valid() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(files.clone(), 0, false, true);
        assert!(q.remove(&files[0]));
        assert!(!q.remove(&files[0]));
        assert_eq!(q.restart().as_deref(), Some(files[1].as_str()));
    }

    #[test]
    fn shuffle_visits_all() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(files.clone(), 0, true, true);
        q.restart();
        let mut seen: Vec<String> = (0..3).filter_map(|_| q.next_file()).collect();
        seen.sort();
        assert_eq!(seen, files);
    }

    #[test]
    fn restart_goes_to_top() {
        let (_dir, files) = fixture();
        let mut q = RotationQueue::new(files.clone(), -1, false, true);
        q.next_file();
        q.next_file();
        assert_eq!(q.restart().as_deref(), Some(files[0].as_str()));
    }

    #[test]
    fn prune_missing_drops_gone() {
        let (dir, files) = fixture();
        let mut q = RotationQueue::new(
            vec![files[0].clone(), "/definitely/not/here.mp4".to_string()],
            5,
            false,
            true,
        );
        assert_eq!(q.prune_missing(), 1);
        assert_eq!(q.files(), &[files[0].clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
