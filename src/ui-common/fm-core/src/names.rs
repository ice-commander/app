pub fn split_stem_ext(name: &str, is_dir: bool) -> (&str, &str) {
    match (is_dir, name.rfind('.')) {
        (false, Some(i)) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

pub fn first_free(
    name: &str,
    is_dir: bool,
    taken: &[String],
    make: impl Fn(&str, &str, u32) -> String,
) -> String {
    let (stem, ext) = split_stem_ext(name, is_dir);
    (1..u32::MAX)
        .map(|n| make(stem, ext, n))
        .find(|c| !taken.iter().any(|t| t == c))
        .unwrap_or_else(|| name.to_string())
}

pub fn keep_or_number(
    name: &str,
    is_dir: bool,
    taken: &[String],
    make: impl Fn(&str, &str, u32) -> String,
) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_string();
    }
    first_free(name, is_dir, taken, make)
}

const WINDOWS_DEVICES: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];

pub fn windows_refuses(name: &str) -> bool {
    let device = name
        .split_once('.')
        .map_or(name, |(before, _)| before)
        .trim_end_matches(' ');
    let port = device.get(..3).unwrap_or_default();
    let mut number = device.get(3..).unwrap_or_default().chars();
    let numbered_port = (port.eq_ignore_ascii_case("COM") || port.eq_ignore_ascii_case("LPT"))
        && matches!(
            (number.next(), number.next()),
            (Some('0'..='9' | '¹' | '²' | '³'), None)
        );
    let reserved = WINDOWS_DEVICES
        .iter()
        .any(|reserved| device.eq_ignore_ascii_case(reserved));
    let unwritable = |c: char| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*');
    numbered_port || reserved || name.ends_with(['.', ' ']) || name.chars().any(unwritable)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Exact,
    Windows,
    MacOs,
}

impl Comparison {
    pub const THIS_SYSTEM: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(any(target_os = "macos", target_os = "android")) {
        Self::MacOs
    } else {
        Self::Exact
    };
}

fn upcased(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (c, upper.next(), upper.next()) {
        ('\u{1f80}'..='\u{1f87}' | '\u{1f90}'..='\u{1f97}' | '\u{1fa0}'..='\u{1fa7}', ..) => {
            char::from_u32(u32::from(c) + 8).unwrap_or(c)
        }
        ('\u{1fb3}' | '\u{1fc3}' | '\u{1ff3}', ..) => char::from_u32(u32::from(c) + 9).unwrap_or(c),
        (_, Some(one), None) if one.to_lowercase().eq([c]) => one,
        _ => c,
    }
}

pub fn folded(name: &str, by: Comparison) -> std::borrow::Cow<'_, str> {
    match by {
        Comparison::Exact => std::borrow::Cow::Borrowed(name),
        Comparison::Windows => name
            .chars()
            .map(|c| if c <= '\u{ffff}' { upcased(c) } else { c })
            .collect(),
        Comparison::MacOs => {
            let nfd = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd();
            let mut caseless = String::new();
            for c in nfd.normalize(name).chars() {
                match c {
                    'ı' => caseless.push(c),
                    _ => caseless.extend(
                        c.to_lowercase()
                            .flat_map(char::to_uppercase)
                            .flat_map(char::to_lowercase),
                    ),
                }
            }
            std::borrow::Cow::Owned(nfd.normalize(&caseless).into_owned())
        }
    }
}

pub fn one_component(name: &str) -> Result<(), common::AppError> {
    let mut parts = std::path::Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(only)), None)
            if only == std::ffi::OsStr::new(name) && !(cfg!(windows) && windows_refuses(name)) =>
        {
            Ok(())
        }
        _ => Err(common::AppError::Other(format!(
            "\"{name}\" is not a valid file name on this system"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_in_any_script_splits_at_its_last_dot() {
        for (name, stem, ext) in [
            ("звіт.txt", "звіт", ".txt"),
            ("报告.txt", "报告", ".txt"),
            ("تقرير.txt", "تقرير", ".txt"),
            ("σπίτι.tar.gz", "σπίτι.tar", ".gz"),
            ("🎧.mp3", "🎧", ".mp3"),
            ("файл без розширення", "файл без розширення", ""),
            (".прихований", ".прихований", ""),
        ] {
            assert_eq!(split_stem_ext(name, false), (stem, ext), "{name}");
        }
    }

    #[test]
    fn a_copy_of_a_non_latin_file_keeps_its_extension() {
        let taken = vec!["تقرير copy.txt".to_string()];
        let made = first_free("تقرير.txt", false, &taken, |stem, ext, n| {
            if n == 1 {
                format!("{stem} copy{ext}")
            } else {
                format!("{stem} copy {n}{ext}")
            }
        });
        assert_eq!(made, "تقرير copy 2.txt");
    }

    #[test]
    fn a_directory_never_splits_at_a_dot() {
        assert_eq!(split_stem_ext("backup.2024", true), ("backup.2024", ""));
        assert_eq!(split_stem_ext("backup.2024", false), ("backup", ".2024"));
    }

    #[test]
    fn a_leading_dot_is_not_an_extension() {
        assert_eq!(split_stem_ext(".bashrc", false), (".bashrc", ""));
    }

    #[test]
    fn only_the_last_dot_splits() {
        assert_eq!(split_stem_ext("app.tar.gz", false), ("app.tar", ".gz"));
    }

    #[test]
    fn a_free_name_is_kept_as_is() {
        let taken = vec!["b.txt".to_string()];
        assert_eq!(
            keep_or_number("a.txt", false, &taken, |s, e, n| format!("{s} ({n}){e}")),
            "a.txt"
        );
    }

    #[test]
    fn a_taken_name_gets_the_first_free_number() {
        let taken = vec!["a.txt".to_string()];
        assert_eq!(
            keep_or_number("a.txt", false, &taken, |s, e, n| format!("{s} ({n}){e}")),
            "a (1).txt"
        );
    }

    #[test]
    fn counting_skips_names_already_taken() {
        let taken = vec!["a (1).txt".to_string(), "a (2).txt".to_string()];
        let got = first_free("a.txt", false, &taken, |s, e, n| format!("{s} ({n}){e}"));
        assert_eq!(got, "a (3).txt");
    }
}
