//! Resolve per-monitor wallpaper assignments into concrete targets.
//!
//! The settings UI stores `monitor name -> media path`. This module keeps
//! the selection rules pure so they can be tested without a display:
//! only connected monitors with a non-empty assignment become targets,
//! in the same order as the monitor list.

use std::collections::BTreeMap;

/// One connected monitor as the platform layer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorDescriptor {
    pub name: String,
    /// `(x, y, width, height)`, already normalized.
    pub rect: (i32, i32, i32, i32),
}

/// A monitor plus the file it should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorTarget {
    pub name: String,
    pub rect: (i32, i32, i32, i32),
    pub path: String,
}

/// Build the per-monitor target list.
///
/// * assignments for monitors that are not connected are ignored,
/// * empty paths are ignored,
/// * output order follows `monitors`, not map order.
pub fn resolve_targets(
    monitors: &[MonitorDescriptor],
    assignments: &BTreeMap<String, String>,
) -> Vec<MonitorTarget> {
    let mut targets = Vec::new();
    for monitor in monitors {
        let Some(path) = assignments.get(&monitor.name) else {
            continue;
        };
        if path.trim().is_empty() || monitor.rect.2 <= 0 || monitor.rect.3 <= 0 {
            continue;
        }
        targets.push(MonitorTarget {
            name: monitor.name.clone(),
            rect: monitor.rect,
            path: path.clone(),
        });
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, x: i32, y: i32, w: i32, h: i32) -> MonitorDescriptor {
        MonitorDescriptor {
            name: name.to_string(),
            rect: (x, y, w, h),
        }
    }

    #[test]
    fn keeps_monitor_order() {
        let monitors = vec![
            monitor("DP-1", 0, 0, 1920, 1080),
            monitor("HDMI-1", 1920, 0, 2560, 1440),
        ];
        let assignments = BTreeMap::from([
            ("HDMI-1".to_string(), "b.mp4".to_string()),
            ("DP-1".to_string(), "a.mp4".to_string()),
        ]);
        let targets = resolve_targets(&monitors, &assignments);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].name, "DP-1");
        assert_eq!(targets[1].name, "HDMI-1");
    }

    #[test]
    fn skips_missing_and_empty_assignments() {
        let monitors = vec![
            monitor("DP-1", 0, 0, 1920, 1080),
            monitor("DP-2", 1920, 0, 1920, 1080),
            monitor("DP-3", 3840, 0, 1920, 1080),
        ];
        let assignments = BTreeMap::from([
            ("DP-1".to_string(), "a.mp4".to_string()),
            ("DP-2".to_string(), "   ".to_string()),
            ("DP-4".to_string(), "disconnected.mp4".to_string()),
        ]);
        let targets = resolve_targets(&monitors, &assignments);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "DP-1");
    }

    #[test]
    fn empty_inputs_produce_no_targets() {
        assert!(resolve_targets(&[], &BTreeMap::new()).is_empty());
        let monitors = vec![monitor("DP-1", 0, 0, 1920, 1080)];
        assert!(resolve_targets(&monitors, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn degenerate_rect_is_skipped() {
        let monitors = vec![monitor("DP-1", 0, 0, 0, 0)];
        let assignments = BTreeMap::from([("DP-1".to_string(), "a.mp4".to_string())]);
        assert!(resolve_targets(&monitors, &assignments).is_empty());
    }
}
