//! A window a plugin draws, on the file the user asked to look at.
//!
//! The plugin is handed the filesystem the file lives on and the path it lives
//! at — never the bytes — and answers with a document this frontend draws. A
//! file on a disk is read where it lies; one on a server is fetched once, into
//! a directory of the host's own, and swept away when the window closes.

use adw::prelude::*;
use gtk_viewer_ui::{Needs, Payload, ViewerCtx, ViewerPlugin};

pub(super) struct DeclaredPlugin {
    viewer: String,
}

impl DeclaredPlugin {
    pub(super) fn showing(viewer: String) -> DeclaredPlugin {
        DeclaredPlugin { viewer }
    }
}

impl ViewerPlugin for DeclaredPlugin {
    fn needs(&self) -> Needs {
        Needs::Source
    }

    fn build(&self, ctx: &ViewerCtx, payload: Payload) {
        let Payload::Source(staged) = payload else {
            return;
        };
        let source =
            fm_core::host_fs::HostSource::rooted_at(&staged.root, &ctx.provider.fs_id(), "").open();
        let Some(shown) = crate::plugin_view::embed_viewer(&self.viewer, source, &staged.name)
        else {
            fm_core::host_fs::HostSource::close(source);
            ctx.stack.add_named(
                &gtk::Label::new(Some(&crate::i18n::tr("viewer.plugin_failed"))),
                Some("declared"),
            );
            ctx.stack.set_visible_child_name("declared");
            return;
        };
        ctx.stack.add_named(&shown.widget, Some("declared"));
        ctx.stack.set_visible_child_name("declared");
        let crate::plugin_view::Shown {
            close, may_close, ..
        } = shown;

        // The window owns all three: the plugin's view, the filesystem it was
        // let into, and the copy of the file if one had to be made. They go
        // in that order, because the plugin may read while it is closing.
        let held = std::cell::RefCell::new(Some(staged));
        ctx.window.connect_close_request(move |_| {
            // The plugin is asked first: it may have something unfinished in
            // there, and it draws what it wants to say into this same window.
            if !may_close() {
                return gtk::glib::Propagation::Stop;
            }
            // Only this window's own folder: a song started elsewhere plays on.
            if let Some(standing) = held.borrow().as_ref() {
                crate::player::the_one().stop_if_under(&standing.root);
            }
            close();
            fm_core::host_fs::HostSource::close(source);
            held.borrow_mut().take();
            gtk::glib::Propagation::Proceed
        });
    }

    fn window_size(&self) -> (i32, i32) {
        (900, 700)
    }
}
