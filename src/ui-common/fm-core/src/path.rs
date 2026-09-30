const WINDOWS: bool = cfg!(target_os = "windows");

pub fn is_drive_segment(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

/// The single rule for turning breadcrumb segments into the path string a filesystem expects.
/// Every producer must go through here: a display path that disagrees with the level-relative
/// path makes prefix stripping fail, and the two then compose into a doubled path.
/// `windows` is a parameter so both platform behaviours stay under test everywhere.
fn join_with(names: &[&str], windows: bool) -> String {
    if names.is_empty() {
        return "/".to_string();
    }
    let mut path = String::new();
    for (i, seg) in names.iter().enumerate() {
        if i == 0 {
            path = if windows && is_drive_segment(seg) {
                format!("{seg}/")
            } else {
                format!("/{seg}")
            };
        } else {
            // A backslash is a separator on Windows only; elsewhere `dir\` is an ordinary name.
            let already_separated = path.ends_with('/') || (windows && path.ends_with('\\'));
            if !already_separated {
                path.push('/');
            }
            path.push_str(seg);
        }
    }
    path
}

pub fn join_segment_names<S: AsRef<str>>(names: &[S]) -> String {
    let names: Vec<&str> = names.iter().map(|s| s.as_ref()).collect();
    join_with(&names, WINDOWS)
}

/// For user-typed or OS-shaped input, where a backslash is a separator.
pub fn split_path(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
}

/// For strings this module produced. A backslash inside an archive entry name is part of the
/// name, not a separator, so it must survive the round trip.
pub fn split_joined(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_names_build_the_root() {
        let none: [&str; 0] = [];
        assert_eq!(join_segment_names(&none), "/");
        assert_eq!(join_with(&[], true), "/");
        assert_eq!(join_with(&[], false), "/");
    }

    #[test]
    fn plain_names_are_joined_under_a_leading_slash_on_both_platforms() {
        for windows in [true, false] {
            assert_eq!(join_with(&["tmp"], windows), "/tmp");
            assert_eq!(
                join_with(&["home", "ice", "docs"], windows),
                "/home/ice/docs"
            );
        }
    }

    #[test]
    fn unicode_and_spaces_pass_through() {
        for windows in [true, false] {
            assert_eq!(
                join_with(&["дом", "мои файлы", "🎧.mp3"], windows),
                "/дом/мои файлы/🎧.mp3"
            );
        }
    }

    #[test]
    fn scripts_beyond_latin_survive_joining_and_splitting() {
        for (label, names, joined) in [
            (
                "ukrainian",
                vec!["дім", "мої файли", "звіт.txt"],
                "/дім/мої файли/звіт.txt",
            ),
            (
                "chinese",
                vec!["家", "我的文件", "报告.txt"],
                "/家/我的文件/报告.txt",
            ),
            (
                "arabic",
                vec!["منزل", "ملفاتي", "تقرير.txt"],
                "/منزل/ملفاتي/تقرير.txt",
            ),
            (
                "greek",
                vec!["σπίτι", "τα αρχεία μου"],
                "/σπίτι/τα αρχεία μου",
            ),
            (
                "emoji",
                vec!["🎧 music", "track 1.mp3"],
                "/🎧 music/track 1.mp3",
            ),
        ] {
            for windows in [true, false] {
                assert_eq!(join_with(&names, windows), joined, "{label}");
                assert_eq!(split_joined(joined), names, "{label}");
                assert_eq!(
                    join_with(&split_joined(joined), windows),
                    joined,
                    "{label} round trip"
                );
            }
        }
    }

    #[test]
    fn a_drive_letter_roots_the_path_only_on_windows() {
        assert_eq!(join_with(&["C:"], true), "C:/");
        assert_eq!(join_with(&["C:", "Users", "ice"], true), "C:/Users/ice");
        assert_eq!(join_with(&["C:"], false), "/C:");
        assert_eq!(join_with(&["C:", "Users", "ice"], false), "/C:/Users/ice");
    }

    #[test]
    fn a_trailing_backslash_absorbs_the_separator_only_on_windows() {
        assert_eq!(
            join_with(&["home", "dir\\", "file.txt"], true),
            "/home/dir\\file.txt"
        );
        assert_eq!(
            join_with(&["home", "dir\\", "file.txt"], false),
            "/home/dir\\/file.txt"
        );
    }

    #[test]
    fn drive_segments_are_recognised_by_shape_only() {
        assert!(is_drive_segment("C:"));
        assert!(is_drive_segment("z:"));
        assert!(!is_drive_segment("C:\\"));
        assert!(!is_drive_segment("CD:"));
        assert!(!is_drive_segment("1:"));
        assert!(!is_drive_segment(":"));
    }

    #[test]
    fn splitting_accepts_both_separators_and_drops_empties() {
        assert_eq!(split_path("/home//ice/"), vec!["home", "ice"]);
        assert_eq!(split_path("C:\\Users\\ice"), vec!["C:", "Users", "ice"]);
        assert_eq!(split_path("/"), Vec::<&str>::new());
    }

    #[test]
    fn splitting_a_joined_path_keeps_a_backslash_inside_a_component() {
        assert_eq!(split_joined("/dir\\file.txt"), vec!["dir\\file.txt"]);
        assert_eq!(split_joined("/a//b/"), vec!["a", "b"]);
    }

    // A display path must survive the trip through the panel and back to the provider unchanged.
    #[test]
    fn joining_and_splitting_again_is_stable_on_both_platforms() {
        for windows in [true, false] {
            for names in [
                vec!["home", "ice", "docs"],
                vec!["C:", "msys64", "home"],
                vec!["дом", "файлы"],
            ] {
                let joined = join_with(&names, windows);
                assert_eq!(
                    join_with(&split_joined(&joined), windows),
                    joined,
                    "{names:?}"
                );
            }
        }
    }

    // Off Windows a name ending in a backslash is one component and must stay one component.
    #[test]
    fn a_backslash_named_component_is_not_merged_with_its_neighbour() {
        for names in [
            vec!["home", "dir\\", "file.txt"],
            vec!["a", "b\\", "c\\", "d"],
            vec!["архив", "папка\\", "файл.txt"],
        ] {
            let joined = join_with(&names, false);
            assert_eq!(split_joined(&joined), names, "{joined:?}");
        }
    }
}
