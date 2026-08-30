use ratatui::layout::Rect;

pub(crate) fn human_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut s = n as f64;
    let mut i = 0;
    while s >= 1024.0 && i < UNITS.len() - 1 {
        s /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{s:.1}{}", UNITS[i])
    }
}

pub(crate) fn fmt_time(ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(ts as i64, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        _ => String::new(),
    }
}

pub(crate) fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

pub(crate) fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|&b| b == 0)
}

pub(crate) fn to_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split('\n')
        .map(|s| s.trim_end_matches('\r').to_string())
        .collect()
}

pub(crate) fn join_rel(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_below_one_kilobyte_keep_the_exact_byte_count() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1), "1 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn exactly_one_kilobyte_switches_to_the_next_unit() {
        assert_eq!(human_size(1024), "1.0K");
    }

    #[test]
    fn sizes_are_rounded_to_one_decimal() {
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn terabytes_are_the_largest_unit_used() {
        assert_eq!(human_size(1024u64.pow(4)), "1.0T");
        assert_eq!(human_size(1024u64.pow(5)), "1024.0T");
        assert!(human_size(u64::MAX).ends_with('T'));
    }

    #[test]
    fn zero_timestamp_renders_as_an_empty_string() {
        assert_eq!(fmt_time(0), "");
    }

    #[test]
    fn a_real_timestamp_renders_as_date_and_time() {
        let s = fmt_time(1_000_000_000);
        assert_eq!(s.len(), 16);
        assert_eq!(s.matches('-').count(), 2);
        assert_eq!(s.matches(':').count(), 1);
    }

    #[test]
    fn a_timestamp_beyond_the_calendar_range_renders_as_an_empty_string() {
        assert_eq!(fmt_time(i64::MAX as u64), "");
    }

    #[test]
    fn centered_rect_is_centered_inside_the_area() {
        let area = Rect { x: 0, y: 0, width: 100, height: 50 };
        let r = centered_rect(20, 10, area);
        assert_eq!((r.x, r.y, r.width, r.height), (40, 20, 20, 10));
    }

    #[test]
    fn centered_rect_respects_the_area_origin() {
        let area = Rect { x: 7, y: 3, width: 20, height: 10 };
        let r = centered_rect(10, 4, area);
        assert_eq!((r.x, r.y), (12, 6));
    }

    #[test]
    fn centered_rect_never_grows_past_the_area() {
        let area = Rect { x: 0, y: 0, width: 10, height: 4 };
        let r = centered_rect(80, 40, area);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 10, 4));
    }

    #[test]
    fn centered_rect_of_an_empty_area_is_empty() {
        let r = centered_rect(30, 8, Rect { x: 0, y: 0, width: 0, height: 0 });
        assert_eq!((r.width, r.height), (0, 0));
    }

    #[test]
    fn a_nul_byte_makes_the_content_binary() {
        assert!(is_binary(b"abc\0def"));
    }

    #[test]
    fn plain_text_is_not_binary() {
        assert!(!is_binary(b"hello world\n\r\ttabs"));
        assert!(!is_binary("Привет, мир — ok".as_bytes()));
        assert!(!is_binary(b""));
    }

    #[test]
    fn a_nul_past_the_sniffed_prefix_is_not_detected() {
        let mut data = vec![b'a'; 8192];
        data.push(0);
        assert!(!is_binary(&data));
        data[8191] = 0;
        assert!(is_binary(&data));
    }

    #[test]
    fn empty_input_is_a_single_empty_line() {
        assert_eq!(to_lines(b""), vec![String::new()]);
    }

    #[test]
    fn a_trailing_newline_produces_a_trailing_empty_line() {
        assert_eq!(to_lines(b"a\nb\n"), vec!["a", "b", ""]);
    }

    #[test]
    fn carriage_returns_are_stripped_from_line_ends() {
        assert_eq!(to_lines(b"a\r\nb\r\n"), vec!["a", "b", ""]);
        assert_eq!(to_lines(b"only\r"), vec!["only"]);
    }

    #[test]
    fn a_carriage_return_inside_a_line_is_kept() {
        assert_eq!(to_lines(b"a\rb"), vec!["a\rb"]);
    }

    #[test]
    fn invalid_utf8_is_replaced_rather_than_dropped() {
        let lines = to_lines(&[0xff, 0xfe, b'\n', b'x']);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains('\u{fffd}'));
        assert_eq!(lines[1], "x");
    }

    #[test]
    fn joining_onto_the_root_does_not_double_the_slash() {
        assert_eq!(join_rel("/", "file.txt"), "/file.txt");
    }

    #[test]
    fn joining_onto_a_directory_inserts_one_slash() {
        assert_eq!(join_rel("/home/user", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn a_trailing_slash_on_the_base_is_not_duplicated() {
        assert_eq!(join_rel("/home/user/", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn an_empty_base_still_yields_an_absolute_path() {
        assert_eq!(join_rel("", "file.txt"), "/file.txt");
    }

    #[test]
    fn unicode_and_spaces_in_names_survive_joining() {
        assert_eq!(join_rel("/док", "имя файла.txt"), "/док/имя файла.txt");
    }
}
