use ic_model::SysInfo;
use std::sync::{Mutex, OnceLock};
use sysinfo::{CpuExt, CpuRefreshKind, RefreshKind, System, SystemExt};

fn shared() -> &'static Mutex<System> {
    static SYS: OnceLock<Mutex<System>> = OnceLock::new();
    SYS.get_or_init(|| {
        Mutex::new(System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(),
        ))
    })
}

fn get_network_interfaces() -> Vec<String> {
    use network_interface::{NetworkInterface, NetworkInterfaceConfig};
    let mut network_interfaces = Vec::new();
    if let Ok(interfaces) = NetworkInterface::show() {
        for iface in interfaces {
            for addr in iface.addr {
                let ip = match addr {
                    network_interface::Addr::V4(v4) => std::net::IpAddr::V4(v4.ip),
                    network_interface::Addr::V6(v6) => std::net::IpAddr::V6(v6.ip),
                };
                if !ip.is_loopback() {
                    network_interfaces.push(format!("{}: {}", iface.name, ip));
                }
            }
        }
    }
    network_interfaces
}

pub fn get_system_info() -> SysInfo {
    let mut sys = shared().lock().unwrap_or_else(|e| e.into_inner());

    sys.refresh_cpu();
    sys.refresh_memory();

    SysInfo {
        hostname: sys.host_name().unwrap_or_else(|| "N/A".to_string()),
        os_family: std::env::consts::OS.to_string(),
        os_type: sys.name().unwrap_or_else(|| "N/A".to_string()),
        os_version: sys.os_version().unwrap_or_else(|| "N/A".to_string()),
        cpu_arch: std::env::consts::ARCH.to_string(),
        cpu_cores: sys.cpus().len(),
        cpu_usage: sys.global_cpu_info().cpu_usage(),
        total_memory: sys.total_memory(),
        used_memory: sys.used_memory(),
        total_swap: sys.total_swap(),
        used_swap: sys.used_swap(),
        uptime: sys.uptime(),
        network_interfaces: get_network_interfaces(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_info_has_sensible_values() {
        let info = get_system_info();
        assert!(!info.os_family.is_empty(), "os_family is empty");
        assert!(info.cpu_cores > 0, "cpu_cores should be > 0");
        assert!(info.total_memory > 0, "total_memory should be > 0");
    }

    #[test]
    fn a_snapshot_is_cheap_enough_to_poll() {
        use std::time::{Duration, Instant};
        let _ = get_system_info(); // warm-up: the one-off construction is not measured

        let runs = 5;
        let started = Instant::now();
        for _ in 0..runs {
            let _ = get_system_info();
        }
        let each = started.elapsed() / runs;

        assert!(
            each < Duration::from_millis(100),
            "a snapshot took {each:?}; polling this every 2s would burn the peer's CPU. \
             Rebuilding System per call is the usual cause"
        );
    }

    #[test]
    fn repeated_snapshots_keep_every_field_and_move_with_the_machine() {
        let first = get_system_info();

        let mut ballast: Vec<u8> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            ballast.extend(std::iter::repeat(7u8).take(64 * 1024));
        }
        assert!(!ballast.is_empty());
        let second = get_system_info();

        assert!(
            second.uptime >= first.uptime,
            "uptime went backwards: {} then {}",
            first.uptime,
            second.uptime
        );
        assert!(
            second.cpu_usage > 0.0,
            "cpu usage stayed at zero while the machine was busy — no delta is being computed"
        );
        assert_eq!(
            first.cpu_cores, second.cpu_cores,
            "cpu_cores changed between snapshots"
        );
        assert!(
            second.total_memory > 0
                && !second.hostname.is_empty()
                && !second.os_type.is_empty(),
            "a targeted refresh dropped a field that new_all() used to fill"
        );
    }
}
