use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub os: String,
    pub version: String,
    pub app_type: String,
    pub build_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryInfo {
    pub name: String,
    pub entry_type: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SysInfo {
    pub hostname: String,
    #[serde(default)]
    pub os_family: String,
    pub os_type: String,
    pub os_version: String,
    pub cpu_arch: String,
    pub cpu_cores: usize,
    #[serde(default)]
    pub cpu_usage: f32,
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub uptime: u64,
    #[serde(default)]
    pub network_interfaces: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryValueInfo {
    pub name: String,
    pub data: RegistryValueData,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RegistryValueData {
    String(String),
    MultiString(Vec<String>),
    DWord(u32),
    QWord(u64),
    #[serde(with = "serde_bytes")]
    Binary(Vec<u8>),
    ExpandString(String),
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn sorted_keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .expect("expected a JSON object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn sample_device() -> DeviceInfo {
        DeviceInfo {
            id: "3f2a".to_string(),
            name: "workstation".to_string(),
            os: "linux".to_string(),
            version: "1.4.0".to_string(),
            app_type: "gtk".to_string(),
            build_type: "release".to_string(),
        }
    }

    fn sample_sys_info() -> SysInfo {
        SysInfo {
            hostname: "workstation".to_string(),
            os_family: "unix".to_string(),
            os_type: "Arch Linux".to_string(),
            os_version: "rolling".to_string(),
            cpu_arch: "x86_64".to_string(),
            cpu_cores: 16,
            cpu_usage: 37.25,
            total_memory: 33_554_432_000,
            used_memory: 12_884_901_888,
            total_swap: 8_589_934_592,
            used_swap: 0,
            uptime: 987_654,
            network_interfaces: vec!["lo".to_string(), "enp3s0".to_string()],
        }
    }

    #[test]
    fn device_info_default_leaves_every_field_empty() {
        let device = DeviceInfo::default();
        assert_eq!(device.id, "");
        assert_eq!(device.name, "");
        assert_eq!(device.os, "");
        assert_eq!(device.version, "");
        assert_eq!(device.app_type, "");
        assert_eq!(device.build_type, "");
    }

    #[test]
    fn device_info_json_field_names_are_stable() {
        let value = serde_json::to_value(sample_device()).unwrap();
        assert_eq!(
            sorted_keys(&value),
            vec!["app_type", "build_type", "id", "name", "os", "version"]
        );
        assert_eq!(value["app_type"], json!("gtk"));
        assert_eq!(value["build_type"], json!("release"));
        assert_eq!(value["id"], json!("3f2a"));
    }

    #[test]
    fn device_info_round_trips_through_json() {
        let original = sample_device();
        let text = serde_json::to_string(&original).unwrap();
        let restored: DeviceInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(restored.id, original.id);
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.os, original.os);
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.app_type, original.app_type);
        assert_eq!(restored.build_type, original.build_type);
    }

    #[test]
    fn device_info_rejects_json_with_a_missing_field() {
        let text = r#"{"id":"3f2a","name":"n","os":"linux","version":"1","app_type":"gtk"}"#;
        assert!(serde_json::from_str::<DeviceInfo>(text).is_err());
    }

    #[test]
    fn entry_info_json_field_names_are_stable() {
        let entry = EntryInfo {
            name: "notes.txt".to_string(),
            entry_type: "File".to_string(),
            size: 12,
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(sorted_keys(&value), vec!["entry_type", "name", "size"]);
        assert_eq!(value["entry_type"], json!("File"));
        assert_eq!(value["size"], json!(12));
    }

    #[test]
    fn entry_info_round_trips_directories_and_files() {
        for entry_type in ["File", "Directory"] {
            let original = EntryInfo {
                name: "shared".to_string(),
                entry_type: entry_type.to_string(),
                size: 0,
            };
            let restored: EntryInfo =
                serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
            assert_eq!(restored, original);
        }
    }

    #[test]
    fn entry_info_compares_every_field_for_equality() {
        let base = EntryInfo {
            name: "a".to_string(),
            entry_type: "File".to_string(),
            size: 1,
        };
        assert_eq!(base, base.clone());
        assert_ne!(
            base,
            EntryInfo {
                size: 2,
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            EntryInfo {
                entry_type: "Directory".to_string(),
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            EntryInfo {
                name: "b".to_string(),
                ..base.clone()
            }
        );
    }

    #[test]
    fn entry_info_survives_a_maximum_size() {
        let original = EntryInfo {
            name: "huge.img".to_string(),
            entry_type: "File".to_string(),
            size: u64::MAX,
        };
        let text = serde_json::to_string(&original).unwrap();
        assert!(text.contains("18446744073709551615"));
        let restored: EntryInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn entry_info_keeps_unicode_separators_and_empty_names_intact() {
        for name in [
            "",
            "документы",
            "汉字 файл.txt",
            "a/b\\c",
            "  spaced  ",
            "emoji \u{1f600}",
        ] {
            let original = EntryInfo {
                name: name.to_string(),
                entry_type: "File".to_string(),
                size: 3,
            };
            let restored: EntryInfo =
                serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
            assert_eq!(restored, original);
        }
    }

    #[test]
    fn unknown_json_fields_are_ignored_when_reading_an_entry() {
        let text = r#"{"name":"a.txt","entry_type":"File","size":7,"modified":"2026-01-01"}"#;
        let restored: EntryInfo = serde_json::from_str(text).unwrap();
        assert_eq!(restored.name, "a.txt");
        assert_eq!(restored.size, 7);
    }

    #[test]
    fn sys_info_json_field_names_are_stable() {
        let value = serde_json::to_value(sample_sys_info()).unwrap();
        assert_eq!(
            sorted_keys(&value),
            vec![
                "cpu_arch",
                "cpu_cores",
                "cpu_usage",
                "hostname",
                "network_interfaces",
                "os_family",
                "os_type",
                "os_version",
                "total_memory",
                "total_swap",
                "uptime",
                "used_memory",
                "used_swap",
            ]
        );
        assert_eq!(value["cpu_cores"], json!(16));
        assert_eq!(value["network_interfaces"], json!(["lo", "enp3s0"]));
    }

    #[test]
    fn sys_info_round_trips_large_counters_and_fractional_cpu_usage() {
        let original = sample_sys_info();
        let restored: SysInfo =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(restored.hostname, original.hostname);
        assert_eq!(restored.os_family, original.os_family);
        assert_eq!(restored.cpu_cores, original.cpu_cores);
        assert_eq!(restored.cpu_usage, original.cpu_usage);
        assert_eq!(restored.total_memory, original.total_memory);
        assert_eq!(restored.used_swap, original.used_swap);
        assert_eq!(restored.uptime, original.uptime);
        assert_eq!(restored.network_interfaces, original.network_interfaces);
    }

    #[test]
    fn sys_info_cpu_usage_keeps_both_ends_of_the_percent_range() {
        for usage in [0.0f32, 100.0f32] {
            let original = SysInfo {
                cpu_usage: usage,
                ..sample_sys_info()
            };
            let restored: SysInfo =
                serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
            assert_eq!(restored.cpu_usage, usage);
        }
    }

    #[test]
    fn sys_info_fills_in_the_optional_fields_when_they_are_absent() {
        let text = r#"{
            "hostname":"box","os_type":"Linux","os_version":"6.1","cpu_arch":"x86_64",
            "cpu_cores":4,"total_memory":100,"used_memory":50,"total_swap":10,
            "used_swap":1,"uptime":42
        }"#;
        let restored: SysInfo = serde_json::from_str(text).unwrap();
        assert_eq!(restored.os_family, "");
        assert_eq!(restored.cpu_usage, 0.0);
        assert!(restored.network_interfaces.is_empty());
        assert_eq!(restored.cpu_cores, 4);
    }

    #[test]
    fn sys_info_rejects_json_without_a_required_field() {
        let text = r#"{
            "os_type":"Linux","os_version":"6.1","cpu_arch":"x86_64","cpu_cores":4,
            "total_memory":100,"used_memory":50,"total_swap":10,"used_swap":1,"uptime":42
        }"#;
        assert!(serde_json::from_str::<SysInfo>(text).is_err());
    }

    #[test]
    fn sys_info_accepts_an_empty_interface_list() {
        let original = SysInfo {
            network_interfaces: Vec::new(),
            ..sample_sys_info()
        };
        let value = serde_json::to_value(&original).unwrap();
        assert_eq!(value["network_interfaces"], json!([]));
        let restored: SysInfo = serde_json::from_value(value).unwrap();
        assert!(restored.network_interfaces.is_empty());
    }

    #[test]
    fn registry_value_data_uses_externally_tagged_json() {
        let cases = vec![
            (RegistryValueData::String("hello".into()), json!({"String": "hello"})),
            (
                RegistryValueData::MultiString(vec!["a".into(), "b".into()]),
                json!({"MultiString": ["a", "b"]}),
            ),
            (RegistryValueData::DWord(4_294_967_295), json!({"DWord": 4_294_967_295u32})),
            (
                RegistryValueData::QWord(18_446_744_073_709_551_615),
                json!({"QWord": 18_446_744_073_709_551_615u64}),
            ),
            (
                RegistryValueData::ExpandString("%PATH%".into()),
                json!({"ExpandString": "%PATH%"}),
            ),
            (RegistryValueData::Unknown, json!("Unknown")),
        ];
        for (data, expected) in cases {
            assert_eq!(serde_json::to_value(&data).unwrap(), expected);
        }
    }

    #[test]
    fn registry_value_data_round_trips_every_variant() {
        let cases = vec![
            RegistryValueData::String(String::new()),
            RegistryValueData::String("значение".into()),
            RegistryValueData::MultiString(Vec::new()),
            RegistryValueData::MultiString(vec!["one".into(), String::new()]),
            RegistryValueData::DWord(0),
            RegistryValueData::DWord(u32::MAX),
            RegistryValueData::QWord(u64::MAX),
            RegistryValueData::Binary(Vec::new()),
            RegistryValueData::Binary(vec![0, 1, 127, 128, 255]),
            RegistryValueData::ExpandString("%SystemRoot%\\System32".into()),
            RegistryValueData::Unknown,
        ];
        for data in cases {
            let text = serde_json::to_string(&data).unwrap();
            let restored: RegistryValueData = serde_json::from_str(&text).unwrap();
            assert_eq!(
                format!("{:?}", restored),
                format!("{:?}", data),
                "variant did not survive {}",
                text
            );
        }
    }

    #[test]
    fn registry_binary_data_keeps_high_bytes_and_length() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let text = serde_json::to_string(&RegistryValueData::Binary(bytes.clone())).unwrap();
        match serde_json::from_str::<RegistryValueData>(&text).unwrap() {
            RegistryValueData::Binary(restored) => assert_eq!(restored, bytes),
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn registry_value_data_rejects_an_unknown_variant() {
        assert!(serde_json::from_str::<RegistryValueData>(r#"{"Float":1.0}"#).is_err());
    }

    #[test]
    fn registry_value_info_json_field_names_are_stable() {
        let info = RegistryValueInfo {
            name: "Start".to_string(),
            data: RegistryValueData::DWord(2),
        };
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(sorted_keys(&value), vec!["data", "name"]);
        assert_eq!(value["data"], json!({"DWord": 2}));
    }

    #[test]
    fn registry_value_info_round_trips_the_default_unnamed_value() {
        let info = RegistryValueInfo {
            name: String::new(),
            data: RegistryValueData::ExpandString("%TEMP%".into()),
        };
        let restored: RegistryValueInfo =
            serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(restored.name, "");
        match restored.data {
            RegistryValueData::ExpandString(s) => assert_eq!(s, "%TEMP%"),
            other => panic!("expected ExpandString, got {:?}", other),
        }
    }
}
