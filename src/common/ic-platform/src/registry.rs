#[cfg(target_os = "windows")]
use ic_model::{RegistryValueData, RegistryValueInfo};

#[cfg(target_os = "windows")]
pub fn read_registry_keys(path: &str) -> (Vec<String>, Vec<RegistryValueInfo>, Option<String>) {
    use winreg::enums::*;
    use winreg::RegKey;

    if path.is_empty() || path == "/" {
        return (
            vec![
                "HKLM".to_string(),
                "HKCU".to_string(),
                "HKCR".to_string(),
                "HKU".to_string(),
                "HKCC".to_string(),
            ],
            vec![],
            None,
        );
    }

    let parts: Vec<&str> = path.trim_matches('/').splitn(2, '/').collect();
    let root_str = parts[0].to_uppercase();
    let key = match root_str.as_str() {
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKCR" => RegKey::predef(HKEY_CLASSES_ROOT),
        "HKU" => RegKey::predef(HKEY_USERS),
        "HKCC" => RegKey::predef(HKEY_CURRENT_CONFIG),
        _ => return (vec![], vec![], Some("Unknown root key".to_string())),
    };
    let rest = if parts.len() > 1 {
        parts[1].replace("/", "\\")
    } else {
        String::new()
    };

    match key.open_subkey_with_flags(&rest, KEY_READ) {
        Ok(target_key) => {
            let mut subkeys = Vec::new();
            for name in target_key.enum_keys().filter_map(|n| n.ok()) {
                subkeys.push(name);
            }

            let mut values = Vec::new();
            for (name, value) in target_key.enum_values().filter_map(|val| val.ok()) {
                let parse_string = |bytes: &[u8]| -> String {
                    let u16_slice: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&u16_slice)
                        .trim_end_matches('\0')
                        .to_string()
                };

                let v_data = match value.vtype {
                    REG_SZ => RegistryValueData::String(parse_string(&value.bytes)),
                    REG_EXPAND_SZ => RegistryValueData::ExpandString(parse_string(&value.bytes)),
                    REG_MULTI_SZ => {
                        let u16_slice: Vec<u16> = value
                            .bytes
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect();
                        let s = String::from_utf16_lossy(&u16_slice);
                        let v: Vec<String> = s
                            .split('\0')
                            .filter(|x| !x.is_empty())
                            .map(|x| x.to_string())
                            .collect();
                        RegistryValueData::MultiString(v)
                    }
                    REG_DWORD => {
                        if value.bytes.len() >= 4 {
                            let mut arr = [0u8; 4];
                            arr.copy_from_slice(&value.bytes[0..4]);
                            RegistryValueData::DWord(u32::from_le_bytes(arr))
                        } else {
                            RegistryValueData::Unknown
                        }
                    }
                    REG_QWORD => {
                        if value.bytes.len() >= 8 {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(&value.bytes[0..8]);
                            RegistryValueData::QWord(u64::from_le_bytes(arr))
                        } else {
                            RegistryValueData::Unknown
                        }
                    }
                    REG_BINARY => RegistryValueData::Binary(value.bytes),
                    _ => RegistryValueData::Unknown,
                };
                values.push(RegistryValueInfo { name, data: v_data });
            }

            (subkeys, values, None)
        }
        Err(e) => (vec![], vec![], Some(e.to_string())),
    }
}

#[cfg(target_os = "windows")]
pub fn delete_registry_entry(
    path: &str,
    is_key: bool,
    value_name: Option<String>,
) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let parts: Vec<&str> = path.trim_matches('/').splitn(2, '/').collect();

    if parts.is_empty() || path == "/" {
        return Err("Cannot delete root key".to_string());
    }

    let root_str = parts[0].to_uppercase();
    let key = match root_str.as_str() {
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKCR" => RegKey::predef(HKEY_CLASSES_ROOT),
        "HKU" => RegKey::predef(HKEY_USERS),
        "HKCC" => RegKey::predef(HKEY_CURRENT_CONFIG),
        _ => RegKey::predef(HKEY_CURRENT_USER),
    };
    let rest = if parts.len() > 1 {
        parts[1].replace("/", "\\")
    } else {
        String::new()
    };

    if is_key {
        if let Some(last_slash) = rest.rfind('\\') {
            let parent = &rest[0..last_slash];
            let key_to_del = &rest[last_slash + 1..];
            match key.open_subkey_with_flags(parent, KEY_WRITE | KEY_READ) {
                Ok(parent_key) => match parent_key.delete_subkey(key_to_del) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            }
        } else {
            match key.delete_subkey(&rest) {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    } else {
        if let Some(v_name) = value_name {
            match key.open_subkey_with_flags(&rest, KEY_WRITE | KEY_READ) {
                Ok(target_key) => match target_key.delete_value(v_name) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("Value name not provided for value deletion".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
pub fn set_registry_value(
    path: &str,
    value_name: &str,
    value_data: &RegistryValueData,
) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let parts: Vec<&str> = path.trim_matches('/').splitn(2, '/').collect();
    if parts.is_empty() || path == "/" {
        return Err("Cannot set value on root".to_string());
    }

    let root_str = parts[0].to_uppercase();
    let key = match root_str.as_str() {
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKCR" => RegKey::predef(HKEY_CLASSES_ROOT),
        "HKU" => RegKey::predef(HKEY_USERS),
        "HKCC" => RegKey::predef(HKEY_CURRENT_CONFIG),
        _ => RegKey::predef(HKEY_CURRENT_USER),
    };

    let rest = if parts.len() > 1 {
        parts[1].replace("/", "\\")
    } else {
        String::new()
    };

    match key.open_subkey_with_flags(&rest, KEY_WRITE | KEY_READ) {
        Ok(target_key) => {
            let reg_res = match value_data {
                RegistryValueData::String(s) => target_key.set_value(value_name, s),
                RegistryValueData::DWord(d) => target_key.set_value(value_name, d),
                RegistryValueData::QWord(q) => target_key.set_value(value_name, q),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unsupported type for writing",
                )),
            };
            match reg_res {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}
