use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folder: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

impl Connection {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.settings.get(key).map(|s| s.as_str())
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Some("true") | Some("1"))
    }

    pub fn map_secrets(&mut self, keys: &[String], mut f: impl FnMut(&str) -> Option<String>) {
        for key in keys {
            let Some(current) = self.settings.get(key).cloned() else {
                continue;
            };
            if let Some(mapped) = f(&current) {
                self.settings.insert(key.clone(), mapped);
            }
        }
    }

    pub fn number(&self, key: &str) -> Option<u16> {
        self.get(key).and_then(|v| v.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_survives_a_json_round_trip() {
        let c = Connection {
            id: "webdav:files".to_string(),
            name: "files".to_string(),
            folder: None,
            kind: "webdav".to_string(),
            settings: BTreeMap::from([("url".to_string(), "https://x".to_string())]),
        };
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Connection>(&text).unwrap(), c);
    }
}
