use std::sync::Once;

static INIT: Once = Once::new();

fn ensure() {
    INIT.call_once(|| {
        ic_i18n::register_locales!("../locales");
    });
}

pub fn tr(key: &str) -> String {
    ensure();
    ic_i18n::tr(key)
}

#[allow(dead_code)]
pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    ensure();
    ic_i18n::trf(key, args)
}

#[cfg(test)]
mod tests {
    /// A phrase forgotten in one catalogue shows the key itself, only to those who read it.
    #[test]
    fn every_catalogue_carries_the_same_keys() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut catalogues: Vec<(String, std::collections::BTreeSet<String>)> = Vec::new();
        for entry in std::fs::read_dir(&folder).expect("the catalogues are beside the crate") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable");
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
            let keys = parsed
                .as_object()
                .unwrap_or_else(|| panic!("{} is not an object", path.display()))
                .keys()
                .cloned()
                .collect();
            let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            catalogues.push((name, keys));
        }
        assert!(catalogues.len() > 1, "there should be catalogues to compare");

        let (ref first_name, ref first) = catalogues[0];
        for (name, keys) in &catalogues[1..] {
            let missing: Vec<&String> = first.difference(keys).collect();
            let extra: Vec<&String> = keys.difference(first).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{name}.json differs from {first_name}.json — missing: {missing:?}, extra: {extra:?}"
            );
        }
    }
}
