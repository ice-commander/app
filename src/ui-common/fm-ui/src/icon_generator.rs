#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    Executable,
    Developer,
    Media,
    Photo,
    Document,
    Archive,
    ConfigText,
}

impl FileType {
    pub fn colors(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            FileType::Executable => ("#f78f8f", "#c74343", "#ffffff", "#ffffff"),
            FileType::Developer => ("#bae0bd", "#5e9c76", "#ffffff", "#5e9c76"),
            FileType::Media => ("#ffeea3", "#ba9b48", "#ffffff", "#ba9b48"),
            FileType::Photo => ("#869ce8", "#4e64b5", "#ffffff", "#ffffff"),
            FileType::Document => ("#ffffff", "#c74343", "#ffd9d9", "#c74343"),
            FileType::Archive => ("#ffc49c", "#a16a4a", "#ffffff", "#a16a4a"),
            FileType::ConfigText => ("#dcd5f2", "#8b75a1", "#ffffff", "#8b75a1"),
        }
    }
}

use std::collections::HashMap;

use once_cell::sync::Lazy;
use std::sync::RwLock;

static GLOBAL_SVG_CACHE: Lazy<RwLock<HashMap<(String, FileType, u32), String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub const ICON_TEXT_MIN_SIZE: u32 = 28;

pub fn generate_svg_icon(text: &str, file_type: FileType, size: u32) -> String {
    let key = (text.to_string(), file_type, size);
    if let Some(svg) = {
        let cache = GLOBAL_SVG_CACHE.read().unwrap();
        cache.get(&key).cloned()
    } {
        return svg;
    }

    let (bg, border, fold, text_color) = file_type.colors();

    let text_element = if size < ICON_TEXT_MIN_SIZE {
        String::new()
    } else {
        let font_size = match text.len() {
            0..=3 => "19px",
            4 => "15px",
            _ => "12px",
        };

        if size <= 30 {
            format!(
                r##"<text x="40" y="49" fill="{}" font-family="sans-serif" font-weight="900" font-size="{}" text-anchor="middle" text-rendering="geometricPrecision">{}</text>"##,
                text_color, font_size, text
            )
        } else if text_color == "#ffffff" {
            format!(
                r##"<text x="40" y="48" fill="{}" stroke="{}" stroke-width="4" stroke-linejoin="round" font-family="sans-serif" font-weight="900" font-size="{}" text-anchor="middle" text-rendering="geometricPrecision">{}</text>
  <text x="40" y="48" fill="#ffffff" font-family="sans-serif" font-weight="900" font-size="{}" text-anchor="middle" text-rendering="geometricPrecision">{}</text>"##,
                border, border, font_size, text, font_size, text
            )
        } else {
            format!(
                r##"<text x="40" y="48" fill="{}" font-family="sans-serif" font-weight="900" font-size="{}" text-anchor="middle" text-rendering="geometricPrecision">{}</text>"##,
                text_color, font_size, text
            )
        }
    };

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80" width="{}px" height="{}px">
  <path fill="{}" d="M12.5 75.5L12.5 4.5 49.793 4.5 67.5 22.207 67.5 75.5z"/>
  <path fill="{}" d="M49.586,5L67,22.414V75H13V5H49.586 M50,4H12v72h56V22L50,4L50,4z"/>
  <path fill="{}" d="M49.5 22.5L49.5 4.5 49.793 4.5 67.5 22.207 67.5 22.5z"/>
  <path fill="{}" d="M50,5.414L66.586,22H50V5.414 M50,4h-1v19h19v-1L50,4L50,4z"/>
  {}
</svg>"##,
        size, size, bg, border, fold, border, text_element
    );

    {
        let mut cache = GLOBAL_SVG_CACHE.write().unwrap();
        cache.insert(key, svg.clone());
    }

    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svg_generation() {
        let svg = generate_svg_icon("TEST", FileType::Developer, 30);
        assert!(svg.contains("width=\"30px\""));
        assert!(svg.contains("height=\"30px\""));
        assert!(svg.contains("#bae0bd")); // bg
        assert!(svg.contains("#5e9c76")); // border
        assert!(svg.contains("TEST"));
    }

    #[test]
    fn small_icons_drop_the_label() {
        let big = generate_svg_icon("JS", FileType::Developer, ICON_TEXT_MIN_SIZE);
        assert!(
            big.contains("<text"),
            "label expected at size {ICON_TEXT_MIN_SIZE}"
        );
        assert!(big.contains("JS"));
        let tiny = generate_svg_icon("JS", FileType::Developer, ICON_TEXT_MIN_SIZE - 2);
        assert!(
            !tiny.contains("<text"),
            "label must be dropped below the threshold"
        );
        assert!(tiny.contains("#bae0bd"));
    }

    #[test]
    fn the_label_font_shrinks_as_the_label_grows() {
        let short = generate_svg_icon("TXT", FileType::ConfigText, 64);
        let four = generate_svg_icon("TBZ2", FileType::ConfigText, 64);
        let long = generate_svg_icon("LONGER", FileType::ConfigText, 64);
        assert!(short.contains("font-size=\"19px\""), "{short}");
        assert!(four.contains("font-size=\"15px\""), "{four}");
        assert!(long.contains("font-size=\"12px\""), "{long}");
    }

    #[test]
    fn white_labels_get_an_outline_only_above_the_list_size() {
        let crisp = generate_svg_icon("RAW", FileType::Photo, 30);
        assert_eq!(crisp.matches("<text").count(), 1);
        assert!(!crisp.contains("stroke-width"));

        let outlined = generate_svg_icon("RAW", FileType::Photo, 31);
        assert_eq!(outlined.matches("<text").count(), 2);
        assert!(outlined.contains("stroke-width=\"4\""));
    }

    #[test]
    fn dark_labels_never_get_an_outline() {
        let doc = generate_svg_icon("PDF", FileType::Document, 64);
        assert_eq!(doc.matches("<text").count(), 1);
        assert!(!doc.contains("stroke-width"));
        assert!(doc.contains("fill=\"#c74343\""));
    }

    #[test]
    fn the_cache_key_covers_text_type_and_size() {
        let base = generate_svg_icon("ZIP", FileType::Archive, 30);
        assert_eq!(base, generate_svg_icon("ZIP", FileType::Archive, 30));
        assert_ne!(base, generate_svg_icon("ZIP", FileType::Archive, 64));
        assert_ne!(base, generate_svg_icon("ZIP", FileType::Document, 30));
        assert_ne!(base, generate_svg_icon("RAR", FileType::Archive, 30));
    }

    #[test]
    fn the_requested_size_is_written_into_both_dimensions() {
        let svg = generate_svg_icon("GO", FileType::Developer, 128);
        assert!(svg.contains("width=\"128px\""));
        assert!(svg.contains("height=\"128px\""));
        assert!(svg.contains("viewBox=\"0 0 80 80\""));
    }

    #[test]
    fn every_file_type_has_its_own_background() {
        let types = [
            FileType::Executable,
            FileType::Developer,
            FileType::Media,
            FileType::Photo,
            FileType::Document,
            FileType::Archive,
            FileType::ConfigText,
        ];
        let mut backgrounds: Vec<&str> = types.iter().map(|t| t.colors().0).collect();
        backgrounds.sort_unstable();
        let count = backgrounds.len();
        backgrounds.dedup();
        assert_eq!(backgrounds.len(), count, "two file types share a background");
    }

    #[test]
    fn an_empty_label_still_produces_a_well_formed_icon() {
        let svg = generate_svg_icon("", FileType::Media, 64);
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("#ffeea3"));
    }
}

