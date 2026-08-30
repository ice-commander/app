use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

pub const LANGS: &[(&str, &str)] = &[
    ("en", "English"),
    ("ru", "Русский"),
    ("pl", "Polski"),
    ("cs", "Čeština"),
    ("sk", "Slovenčina"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("uk", "Українська"),
    ("it", "Italiano"),
    ("fr", "Français"),
    ("ro", "Română"),
    ("hu", "Magyar"),
    ("be", "Беларуская"),
    ("bg", "Български"),
    ("sr", "Српски"),
];
const DEFAULT: usize = 0;

static CURRENT: AtomicUsize = AtomicUsize::new(DEFAULT);

fn store() -> &'static RwLock<HashMap<String, HashMap<String, String>>> {
    static S: OnceLock<RwLock<HashMap<String, HashMap<String, String>>>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn register(dicts: &[(&str, &str)]) {
    let mut s = store().write().unwrap();
    for (lang, json) in dicts {
        let parsed: HashMap<String, String> = match serde_json::from_str(json) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("i18n: bad dictionary for `{lang}`: {e}");
                continue;
            }
        };
        let entry = s.entry((*lang).to_string()).or_default();
        entry.extend(parsed);
    }
}

#[macro_export]
macro_rules! register_locales {
    ($dir:literal) => {
        $crate::register(&[
            ("en", include_str!(concat!($dir, "/en.json"))),
            ("ru", include_str!(concat!($dir, "/ru.json"))),
            ("pl", include_str!(concat!($dir, "/pl.json"))),
            ("cs", include_str!(concat!($dir, "/cs.json"))),
            ("sk", include_str!(concat!($dir, "/sk.json"))),
            ("de", include_str!(concat!($dir, "/de.json"))),
            ("es", include_str!(concat!($dir, "/es.json"))),
            ("uk", include_str!(concat!($dir, "/uk.json"))),
            ("it", include_str!(concat!($dir, "/it.json"))),
            ("fr", include_str!(concat!($dir, "/fr.json"))),
            ("ro", include_str!(concat!($dir, "/ro.json"))),
            ("hu", include_str!(concat!($dir, "/hu.json"))),
            ("be", include_str!(concat!($dir, "/be.json"))),
            ("bg", include_str!(concat!($dir, "/bg.json"))),
            ("sr", include_str!(concat!($dir, "/sr.json"))),
        ]);
    };
}

pub fn current_lang() -> &'static str {
    LANGS
        .get(CURRENT.load(Ordering::Relaxed))
        .map(|(c, _)| *c)
        .unwrap_or(LANGS[DEFAULT].0)
}

pub fn set_lang(code: &str) {
    let idx = LANGS
        .iter()
        .position(|(c, _)| *c == code)
        .unwrap_or(DEFAULT);
    CURRENT.store(idx, Ordering::Relaxed);
}

pub fn tr(key: &str) -> String {
    let s = store().read().unwrap();
    let cur = current_lang();
    if let Some(v) = s.get(cur).and_then(|m| m.get(key)) {
        return v.clone();
    }
    if let Some(v) = s.get("en").and_then(|m| m.get(key)) {
        return v.clone();
    }
    key.to_string()
}

pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    let template = tr(key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template.as_str();
    while let Some(pos) = rest.find("%{") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[pos..]);
            return out;
        };
        let name = &after[..close];
        match args.iter().find(|(n, _)| *n == name) {
            Some((_, val)) => out.push_str(val),
            None => {
                out.push_str("%{");
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_lookup_and_fallback() {
        register(&[
            ("en", r#"{"a.hello":"Hello","a.bye":"Bye"}"#),
            ("ru", r#"{"a.hello":"Привет"}"#),
        ]);
        set_lang("ru");
        assert_eq!(tr("a.hello"), "Привет");
        assert_eq!(tr("a.bye"), "Bye");
        assert_eq!(tr("a.missing"), "a.missing");
        set_lang("en");
        assert_eq!(tr("a.hello"), "Hello");
    }

    #[test]
    fn interpolation() {
        register(&[("en", r#"{"b.msg":"Added %{added}, updated %{updated}."}"#)]);
        set_lang("en");
        assert_eq!(
            trf("b.msg", &[("added", "3"), ("updated", "2")]),
            "Added 3, updated 2."
        );
        assert_eq!(trf("b.msg", &[("added", "3")]), "Added 3, updated %{updated}.");
    }
}
