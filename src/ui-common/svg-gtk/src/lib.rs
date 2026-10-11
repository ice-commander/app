//! Every SVG the application shows is drawn here, one way on every system: resvg, compiled in, needs neither an image
//! loader beside the application nor a particular GTK. What plugins bring is SVG text as well, so their buttons and
//! the application's own come out alike.
//!
//! An icon, once drawn, is kept by what it is — a hash of its text — and the size it was drawn at: the same SVG from
//! wherever it comes is drawn once. A user's own pictures are not kept; there is no end to those.

use gtk::gdk;
use gtk::prelude::*;
use resvg::{tiny_skia, usvg};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// Drawn at twice the size shown, so it stays sharp on a high-density screen.
const DENSITY: u32 = 2;
/// A picture is drawn no larger than this on its longer side.
const LARGEST: u32 = 4096;

/// Whether these bytes are SVG rather than a raster picture: text that opens an `<svg>` element near its start.
pub fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with('<') && head.contains("<svg")
}

/// The system's fonts, read once for every thread: text inside an SVG is drawn with them.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

fn parse(svg: &[u8]) -> Option<usvg::Tree> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(svg, &options).ok()
}

/// Drawn with its longer side `longest` pixels, its proportions kept.
fn draw(tree: &usvg::Tree, longest: u32) -> Option<tiny_skia::Pixmap> {
    let size = tree.size();
    let scale = longest.max(1) as f32 / size.width().max(size.height());
    let width = ((size.width() * scale).round() as u32).max(1);
    let height = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

fn texture_of(pixmap: tiny_skia::Pixmap) -> gdk::Texture {
    let (width, height) = (pixmap.width(), pixmap.height());
    let bytes = gtk::glib::Bytes::from_owned(pixmap.take());
    gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &bytes,
        width as usize * 4,
    )
    .upcast()
}

fn identity(svg: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    hasher.finish()
}

thread_local! {
    static DRAWN: RefCell<HashMap<(u64, u32), gdk::Texture>> = RefCell::new(HashMap::new());
}

/// An icon fitted into `size` × `size`: drawn the first time it is asked for, kept after.
pub fn icon(svg: &[u8], size: u32) -> Option<gdk::Texture> {
    let key = (identity(svg), size);
    if let Some(held) = DRAWN.with(|drawn| drawn.borrow().get(&key).cloned()) {
        return Some(held);
    }
    let texture = texture_of(draw(&parse(svg)?, size.max(1) * DENSITY)?);
    DRAWN.with(|drawn| drawn.borrow_mut().insert(key, texture.clone()));
    Some(texture)
}

/// An icon as an image that takes `size` pixels.
pub fn icon_image(svg: &[u8], size: u32) -> Option<gtk::Image> {
    let image = gtk::Image::from_paintable(Some(&icon(svg, size)?));
    image.set_pixel_size(size as i32);
    Some(image)
}

/// A picture at its own size, for a file being viewed or a picture in a plugin's window. Not kept.
pub fn picture(svg: &[u8]) -> Option<gdk::Texture> {
    let tree = parse(svg)?;
    let natural = tree.size().width().max(tree.size().height()).ceil() as u32;
    Some(texture_of(draw(
        &tree,
        (natural * DENSITY).clamp(1, LARGEST),
    )?))
}

/// A thumbnail as PNG, its longer side `size` pixels. Touches nothing of GTK, so a worker thread can make it.
pub fn thumbnail_png(svg: &[u8], size: u32) -> Option<Vec<u8>> {
    draw(&parse(svg)?, size)?.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
        <rect width="40" height="20" fill="red"/></svg>"#;

    #[test]
    fn svg_is_told_from_a_raster_picture() {
        assert!(is_svg(WIDE));
        assert!(is_svg(
            b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"<html><body></body></html>"));
    }

    #[test]
    fn a_drawing_keeps_its_proportions_and_fills_with_colour() {
        let drawn = draw(&parse(WIDE).unwrap(), 64).unwrap();
        assert_eq!((drawn.width(), drawn.height()), (64, 32));
        let middle = drawn.pixel(32, 16).unwrap();
        assert_eq!((middle.red(), middle.alpha()), (255, 255));
    }

    #[test]
    fn a_thumbnail_is_a_png() {
        let png = thumbnail_png(WIDE, 256).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert!(thumbnail_png(b"not an svg", 256).is_none());
    }
}
