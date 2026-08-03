//! CPU, memory, and disk snapshots via sysinfo.

use netvan_core::types::{CpuSnapshot, DiskKind, DiskSnapshot, MemorySnapshot};
use parking_lot::Mutex;
use sysinfo::{CpuRefreshKind, DiskKind as SysDiskKind, Disks, MemoryRefreshKind, RefreshKind, System};

pub struct SystemMetricsCollector {
    system: Mutex<System>,
    disks: Mutex<Disks>,
    warmed_up: Mutex<bool>,
}

impl SystemMetricsCollector {
    pub fn new() -> Self {
        let refresh = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything());
        let mut system = System::new_with_specifics(refresh);
        // Warm-up so the first cpu_usage is not a false zero.
        system.refresh_cpu_specifics(CpuRefreshKind::everything());
        system.refresh_memory_specifics(MemoryRefreshKind::everything());
        let disks = Disks::new_with_refreshed_list();
        Self {
            system: Mutex::new(system),
            disks: Mutex::new(disks),
            warmed_up: Mutex::new(false),
        }
    }

    pub fn refresh_live(&self) {
        let mut system = self.system.lock();
        system.refresh_cpu_specifics(CpuRefreshKind::everything());
        system.refresh_memory_specifics(MemoryRefreshKind::everything());
        let mut disks = self.disks.lock();
        disks.refresh(true);
        let mut warmed = self.warmed_up.lock();
        if !*warmed {
            // Second refresh after a short delay is handled by the next tick;
            // mark warmed so we still expose data after the first real interval.
            *warmed = true;
        }
    }

    pub fn cpu_snapshot(&self) -> CpuSnapshot {
        let system = self.system.lock();
        let cpus = system.cpus();
        let first = cpus.first();
        CpuSnapshot {
            brand: first.map(|c| c.brand().to_string()).unwrap_or_default(),
            vendor_id: first.map(|c| c.vendor_id().to_string()).unwrap_or_default(),
            physical_cores: system.physical_core_count().map(|n| n as u32),
            logical_cores: cpus.len() as u32,
            frequency_mhz: first.map(|c| c.frequency()).filter(|&f| f > 0),
            utilization: system.global_cpu_usage() as f64,
            per_core: cpus.iter().map(|c| c.cpu_usage() as f64).collect(),
        }
    }

    pub fn memory_snapshot(&self) -> MemorySnapshot {
        let system = self.system.lock();
        let total = system.total_memory();
        let used = system.used_memory();
        let available = system.available_memory();
        let utilization = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        MemorySnapshot {
            total_bytes: total,
            used_bytes: used,
            available_bytes: available,
            utilization,
        }
    }

    pub fn disk_snapshots(&self, kind_filter: Option<DiskKind>) -> Vec<DiskSnapshot> {
        let disks = self.disks.lock();
        let mut out = Vec::new();
        for disk in disks.list() {
            if disk.is_removable() {
                continue;
            }
            let kind = match disk.kind() {
                SysDiskKind::SSD => DiskKind::Ssd,
                SysDiskKind::HDD => DiskKind::Hdd,
                _ => continue, // skip unknown / unclassified
            };
            if let Some(filter) = kind_filter {
                if kind != filter {
                    continue;
                }
            }
            let total = disk.total_space();
            if total == 0 {
                continue;
            }
            let available = disk.available_space();
            let used = total.saturating_sub(available);
            let utilization = (used as f64 / total as f64) * 100.0;
            let mount = disk.mount_point().to_string_lossy().to_string();
            let name = disk.name().to_string_lossy().to_string();
            let id = if mount.is_empty() {
                name.clone()
            } else {
                mount.clone()
            };
            out.push(DiskSnapshot {
                id,
                name: if name.is_empty() {
                    mount.clone()
                } else {
                    name
                },
                mount_point: mount,
                file_system: disk.file_system().to_string_lossy().to_string(),
                kind,
                total_bytes: total,
                used_bytes: used,
                available_bytes: available,
                utilization,
            });
        }
        out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        out
    }
}

impl Default for SystemMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
