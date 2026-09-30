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
