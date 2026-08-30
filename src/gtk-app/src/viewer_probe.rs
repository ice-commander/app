use std::cell::RefCell;

struct OpenViewer {
    path: String,
    mode: String,
    buffer: gtk::TextBuffer,
}

thread_local! {
    static OPEN: RefCell<Option<OpenViewer>> = const { RefCell::new(None) };
}

pub fn set_open(path: &str, mode: &str, buffer: gtk::TextBuffer) {
    OPEN.with(|slot| {
        *slot.borrow_mut() = Some(OpenViewer {
            path: path.to_string(),
            mode: mode.to_string(),
            buffer,
        })
    });
}

pub fn clear() {
    OPEN.with(|slot| *slot.borrow_mut() = None);
}

pub fn content() -> Option<(String, String, String)> {
    use gtk::prelude::TextBufferExt;
    OPEN.with(|slot| {
        slot.borrow().as_ref().map(|v| {
            let (start, end) = v.buffer.bounds();
            let text = v.buffer.text(&start, &end, false).to_string();
            (v.path.clone(), v.mode.clone(), text)
        })
    })
}

pub struct Probe;

impl gtk_viewer_ui::ViewerObserver for Probe {
    fn opened(&self, path: &str, mode: &str, buffer: &gtk::TextBuffer) {
        set_open(path, mode, buffer.clone());
    }

    fn closed(&self) {
        clear();
        crate::api::notify_viewer_closed();
    }
}
