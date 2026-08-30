use gtk_viewer_ui::{Needs, Payload, ViewerCtx, ViewerPlugin};

pub(super) struct PdfPlugin;

impl ViewerPlugin for PdfPlugin {
    fn needs(&self) -> Needs {
        Needs::Bytes
    }

    fn build(&self, ctx: &ViewerCtx, payload: Payload) {
        let Payload::Bytes(bytes) = payload else {
            return;
        };
        super::pdf::build_pdf_content(ctx, bytes);
    }

    fn window_size(&self) -> (i32, i32) {
        (850, 900)
    }
}

pub(super) struct AudioPlugin;

impl ViewerPlugin for AudioPlugin {
    fn needs(&self) -> Needs {
        Needs::Nothing
    }

    fn build(&self, ctx: &ViewerCtx, _payload: Payload) {
        super::audio::build_audio_content(ctx);
    }

    fn window_size(&self) -> (i32, i32) {
        (460, 520)
    }
}
