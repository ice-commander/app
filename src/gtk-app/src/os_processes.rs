use sysinfo::{PidExt, ProcessExt, System, SystemExt};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_list_has_expected_columns() {
        let (columns, _data) = get_process_list();
        assert!(columns.contains(&"pid".to_string()));
        assert!(columns.contains(&"name".to_string()));
        assert!(columns.contains(&"memory".to_string()));
        assert!(columns.contains(&"cpu_usage".to_string()));
    }

    #[test]
    fn kill_nonexistent_pid_returns_err() {
        let result = kill_process(u32::MAX);
        assert!(result.is_err());
        assert!(
            result.err().unwrap().contains("not found"),
            "Expected 'not found' in error"
        );
    }
}

pub fn get_process_list() -> (Vec<String>, Vec<(u32, String, u64, f32)>) {
    let mut sys = System::new_all();
    sys.refresh_processes();

    let columns = vec![
        "pid".to_string(),
        "name".to_string(),
        "memory".to_string(),
        "cpu_usage".to_string(),
    ];
    let mut data = Vec::new();
    for (pid, process) in sys.processes() {
        data.push((
            pid.as_u32(),
            process.name().to_string(),
            process.memory(),
            process.cpu_usage(),
        ));
    }

    data.sort_by_key(|a| a.1.to_lowercase());
    (columns, data)
}

pub fn kill_process(pid: u32) -> Result<(), String> {
    let mut sys = System::new_all();
    sys.refresh_processes();

    if let Some(process) = sys.processes().values().find(|p| p.pid().as_u32() == pid) {
        if process.kill() {
            Ok(())
        } else {
            Err(format!("Failed to kill PID: {} (Permission denied?)", pid))
        }
    } else {
        Err(format!("Process PID: {} not found.", pid))
    }
}
