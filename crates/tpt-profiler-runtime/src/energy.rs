//! Package energy from the Linux powercap interface (spec §56).
//!
//! The kernel exposes each RAPL package zone as a directory,
//! `/sys/class/powercap/intel-rapl:<n>`, holding a cumulative `energy_uj`
//! counter in microjoules and `max_energy_range_uj`, the value at which the
//! counter wraps. Only top-level zones (one colon) are summed; sub-zones
//! such as `intel-rapl:0:0` are already included in their parent.
//!
//! The counter is system-wide. Work done by any process between two
//! readings is counted, so a reading taken around a workload is an upper
//! bound on that workload's energy and is noisy. Recent kernels make the
//! counter readable only by root; then the readings are unavailable.

use std::path::{Path, PathBuf};

/// Where the powercap zones live on Linux.
pub const POWERCAP_ROOT: &str = "/sys/class/powercap";

/// Top-level RAPL package zone directories under `root`, sorted by name.
#[must_use]
pub fn package_zones(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut zones: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_package_zone_name)
        })
        .collect();
    zones.sort();
    zones
}

/// Whether a directory name is a top-level package zone, `intel-rapl:<n>`.
#[must_use]
pub fn is_package_zone_name(name: &str) -> bool {
    name.strip_prefix("intel-rapl:")
        .is_some_and(|index| !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()))
}

/// Cumulative counter and wrap point of one zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReading {
    /// Zone directory.
    pub zone: PathBuf,
    /// Cumulative energy in microjoules.
    pub energy_uj: u64,
    /// Value at which `energy_uj` wraps back to zero.
    pub max_range_uj: u64,
}

/// Readings of every package zone at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// One reading per zone that could be read.
    pub zones: Vec<ZoneReading>,
}

/// Reads every package zone under `root`. Returns `None` when no zone can
/// be read, which is the case on platforms without powercap and where the
/// counter is restricted.
#[must_use]
pub fn snapshot(root: &Path) -> Option<Snapshot> {
    let zones: Vec<ZoneReading> = package_zones(root)
        .into_iter()
        .filter_map(|zone| {
            let energy_uj = read_number(&zone.join("energy_uj"))?;
            let max_range_uj = read_number(&zone.join("max_energy_range_uj"))?;
            Some(ZoneReading {
                zone,
                energy_uj,
                max_range_uj,
            })
        })
        .collect();
    (!zones.is_empty()).then_some(Snapshot { zones })
}

fn read_number(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Energy used between two readings of one counter, allowing for one wrap.
///
/// A reading lower than the previous one means the counter wrapped, so the
/// distance is taken through `max_range_uj`. If `max_range_uj` is zero the
/// wrap cannot be measured and the delta is zero.
#[must_use]
pub fn delta_uj(before: u64, after: u64, max_range_uj: u64) -> u64 {
    if after >= before {
        after - before
    } else if max_range_uj > before {
        (max_range_uj - before).saturating_add(after)
    } else {
        0
    }
}

/// Total microjoules used between two snapshots, over the zones present in
/// both. Zones that disappeared between the readings are left out.
#[must_use]
pub fn consumed_uj(before: &Snapshot, after: &Snapshot) -> u64 {
    before
        .zones
        .iter()
        .filter_map(|start| {
            after
                .zones
                .iter()
                .find(|end| end.zone == start.zone)
                .map(|end| delta_uj(start.energy_uj, end.energy_uj, start.max_range_uj))
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn temp(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tpt-rapl-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[cfg(unix)]
    fn zone(root: &Path, name: &str, energy: &str, range: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("zone dir");
        std::fs::write(dir.join("energy_uj"), energy).expect("energy");
        std::fs::write(dir.join("max_energy_range_uj"), range).expect("range");
    }

    #[test]
    fn only_top_level_package_zones_are_names_of_interest() {
        assert!(is_package_zone_name("intel-rapl:0"));
        assert!(is_package_zone_name("intel-rapl:12"));
        assert!(
            !is_package_zone_name("intel-rapl:0:0"),
            "sub-zones are inside their parent"
        );
        assert!(!is_package_zone_name("intel-rapl:"));
        assert!(!is_package_zone_name("acpi"));
    }

    #[test]
    fn delta_handles_plain_increase_and_one_wrap() {
        assert_eq!(delta_uj(100, 350, 1_000), 250);
        assert_eq!(
            delta_uj(900, 50, 1_000),
            150,
            "wraps: 100 to the top, then 50"
        );
        assert_eq!(
            delta_uj(900, 50, 0),
            0,
            "no wrap point, so nothing can be trusted"
        );
        assert_eq!(delta_uj(5, 5, 1_000), 0);
    }

    #[test]
    #[cfg(unix)]
    fn snapshots_read_package_zones_and_skip_the_rest() {
        let root = temp("snap");
        zone(&root, "intel-rapl:0", "1000\n", "5000\n");
        zone(&root, "intel-rapl:1", "300\n", "5000\n");
        zone(&root, "intel-rapl:0:0", "999\n", "5000\n");
        std::fs::create_dir_all(root.join("unrelated")).expect("other");

        let found = snapshot(&root).expect("readable");
        let names: Vec<String> = found
            .zones
            .iter()
            .map(|reading| {
                reading
                    .zone
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["intel-rapl:0", "intel-rapl:1"]);
        assert_eq!(snapshot(&root.join("missing")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn consumption_is_summed_across_zones_present_in_both_readings() {
        let root = temp("sum");
        zone(&root, "intel-rapl:0", "1000", "5000");
        zone(&root, "intel-rapl:1", "300", "5000");
        let before = snapshot(&root).expect("before");
        zone(&root, "intel-rapl:0", "1400", "5000");
        zone(&root, "intel-rapl:1", "350", "5000");
        let after = snapshot(&root).expect("after");
        assert_eq!(consumed_uj(&before, &after), 450);
        let _ = std::fs::remove_dir_all(&root);
    }
}
