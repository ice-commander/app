pub use ic_i18n::{current_lang, set_lang, tr, trf};

pub fn register() {
    ic_i18n::register_locales!("../locales");
}

#[cfg(test)]
mod tests {
    const LOCALES: &[(&str, &str)] = &[
        ("en", include_str!("../locales/en.json")),
        ("ru", include_str!("../locales/ru.json")),
        ("pl", include_str!("../locales/pl.json")),
        ("cs", include_str!("../locales/cs.json")),
        ("sk", include_str!("../locales/sk.json")),
        ("de", include_str!("../locales/de.json")),
        ("es", include_str!("../locales/es.json")),
        ("uk", include_str!("../locales/uk.json")),
        ("it", include_str!("../locales/it.json")),
        ("fr", include_str!("../locales/fr.json")),
        ("ro", include_str!("../locales/ro.json")),
        ("hu", include_str!("../locales/hu.json")),
        ("be", include_str!("../locales/be.json")),
        ("bg", include_str!("../locales/bg.json")),
        ("sr", include_str!("../locales/sr.json")),
    ];

    type Table = std::collections::BTreeMap<String, String>;

    fn table(raw: &str) -> Table {
        serde_json::from_str(raw).expect("a catalogue is a table of strings")
    }

    /// A key with nothing behind it shows as itself, which is how a language
    /// silently falls back to English in the middle of a window. Every
    /// catalogue answers for the same keys or this fails.
    #[test]
    fn every_language_answers_for_the_same_keys_as_english() {
        let english: std::collections::BTreeSet<String> = table(LOCALES[0].1).into_keys().collect();
        assert!(!english.is_empty());
        for (language, raw) in &LOCALES[1..] {
            let theirs: std::collections::BTreeSet<String> = table(raw).into_keys().collect();
            let missing: Vec<&String> = english.difference(&theirs).collect();
            let extra: Vec<&String> = theirs.difference(&english).collect();
            assert!(
                missing.is_empty(),
                "`{language}` is missing {} keys: {missing:?}",
                missing.len()
            );
            assert!(
                extra.is_empty(),
                "`{language}` carries {} keys english does not: {extra:?}",
                extra.len()
            );
        }
    }

    /// A phrase that carries a `%{name}` the English one does not is a phrase
    /// nobody fills in, and it reaches the user as written.
    #[test]
    fn a_translation_names_the_same_values_the_english_one_does() {
        fn named(text: &str) -> std::collections::BTreeSet<String> {
            let mut found = std::collections::BTreeSet::new();
            let mut rest = text;
            while let Some(at) = rest.find("%{") {
                let after = &rest[at + 2..];
                let Some(end) = after.find('}') else { break };
                found.insert(after[..end].to_string());
                rest = &after[end + 1..];
            }
            found
        }
        let english = table(LOCALES[0].1);
        for (language, raw) in &LOCALES[1..] {
            for (key, phrase) in table(raw) {
                let Some(theirs) = english.get(&key) else {
                    continue;
                };
                assert_eq!(
                    named(&phrase),
                    named(theirs),
                    "`{language}` fills different values into {key}"
                );
            }
        }
    }
}
