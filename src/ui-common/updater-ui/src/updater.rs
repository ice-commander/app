#![allow(deprecated)] // adw::MessageDialog — modernization tracked separately
use adw::prelude::*;
use relm4::prelude::*;

pub struct UpdaterInit {
    pub app_name: String,
    pub current_version: String,
    pub parent: gtk::Window,
}

#[derive(Debug)]
pub enum UpdaterInput {
    UpdateAvailable { version: String },
    UpToDate,
    Downloading { version: String },
    Installing,
    Installed,
    Failed(String),
    DialogClosed,
}

#[derive(Debug)]
pub enum UpdaterOutput {
    Install,
    Declined,
    Restart,
}

enum UpdaterPhase {
    Idle,
    Prompt { version: String },
    Downloading { version: String },
    Installing,
    Installed,
    UpToDate,
    Error(String),
}

pub struct UpdaterModel {
    app_name: String,
    current_version: String,
    parent: gtk::Window,
    phase: UpdaterPhase,
    phase_generation: u64,
}

impl UpdaterModel {
    fn set_phase(&mut self, phase: UpdaterPhase) {
        self.phase = phase;
        self.phase_generation = self.phase_generation.wrapping_add(1);
    }
}

pub struct UpdaterWidgets {
    progress_window: gtk::Window,
    progress_label: gtk::Label,
    rendered_generation: u64,
}

impl SimpleComponent for UpdaterModel {
    type Init = UpdaterInit;
    type Input = UpdaterInput;
    type Output = UpdaterOutput;
    type Root = gtk::Box;
    type Widgets = UpdaterWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let progress_window = gtk::Window::builder()
            .transient_for(&init.parent)
            .modal(true)
            .default_width(320)
            .default_height(140)
            .resizable(false)
            .hide_on_close(true)
            .build();

        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .valign(gtk::Align::Center)
            .build();

        let progress_label = gtk::Label::builder().halign(gtk::Align::Center).build();

        let spinner = gtk::Spinner::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        spinner.start();

        main_box.append(&progress_label);
        main_box.append(&spinner);
        progress_window.set_child(Some(&main_box));

        let model = UpdaterModel {
            app_name: init.app_name,
            current_version: init.current_version,
            parent: init.parent,
            phase: UpdaterPhase::Idle,
            phase_generation: 0,
        };

        let widgets = UpdaterWidgets {
            progress_window,
            progress_label,
            rendered_generation: 0,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            UpdaterInput::UpdateAvailable { version } => {
                self.set_phase(UpdaterPhase::Prompt { version });
            }
            UpdaterInput::UpToDate => self.set_phase(UpdaterPhase::UpToDate),
            UpdaterInput::Downloading { version } => {
                self.set_phase(UpdaterPhase::Downloading { version });
            }
            UpdaterInput::Installing => self.set_phase(UpdaterPhase::Installing),
            UpdaterInput::Installed => self.set_phase(UpdaterPhase::Installed),
            UpdaterInput::Failed(error) => self.set_phase(UpdaterPhase::Error(error)),
            UpdaterInput::DialogClosed => self.set_phase(UpdaterPhase::Idle),
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        if widgets.rendered_generation == self.phase_generation {
            return;
        }
        widgets.rendered_generation = self.phase_generation;

        match &self.phase {
            UpdaterPhase::Downloading { version } => {
                widgets.progress_window.set_title(Some(&crate::i18n::tr("updater.download_dialog_title")));
                widgets.progress_label.set_label(&format!(
                    "{} v{}...",
                    crate::i18n::tr("updater.downloading_label"),
                    version
                ));
                widgets.progress_window.present();
            }
            UpdaterPhase::Installing => {
                widgets.progress_window.set_title(Some(&crate::i18n::tr("updater.install_dialog_title")));
                widgets.progress_label.set_label(&crate::i18n::tr("updater.installing_label"));
                widgets.progress_window.present();
            }
            UpdaterPhase::Idle => widgets.progress_window.set_visible(false),
            UpdaterPhase::Prompt { version } => {
                widgets.progress_window.set_visible(false);
                present_prompt_dialog(&self.parent, &self.app_name, version, &sender);
            }
            UpdaterPhase::UpToDate => {
                widgets.progress_window.set_visible(false);
                present_up_to_date_dialog(
                    &self.parent,
                    &self.app_name,
                    &self.current_version,
                    &sender,
                );
            }
            UpdaterPhase::Installed => {
                widgets.progress_window.set_visible(false);
                present_success_dialog(&self.parent, &sender);
            }
            UpdaterPhase::Error(error) => {
                widgets.progress_window.set_visible(false);
                present_error_dialog(&self.parent, error, &sender);
            }
        }
    }
}

fn present_prompt_dialog(
    parent: &gtk::Window,
    app_name: &str,
    version: &str,
    sender: &ComponentSender<UpdaterModel>,
) {
    let dialog = adw::MessageDialog::builder()
        .heading(&*crate::i18n::tr("updater.available_title"))
        .body(format!(
            "{} {} v{}. {}",
            crate::i18n::tr("updater.available_body_1"),
            app_name,
            version,
            crate::i18n::tr("updater.available_body_2")
        ))
        .transient_for(parent)
        .build();

    dialog.add_response(
        "later",
        &crate::i18n::tr("updater.not_now_btn"),
    );
    dialog.add_response(
        "install",
        &crate::i18n::tr("updater.install_btn"),
    );
    dialog.set_response_appearance("install", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("install"));

    let sender = sender.clone();
    dialog.connect_response(None, move |dlg, response| {
        dlg.close();
        if response == "install" {
            let _ = sender.output(UpdaterOutput::Install);
        } else {
            let _ = sender.output(UpdaterOutput::Declined);
        }
        sender.input(UpdaterInput::DialogClosed);
    });
    dialog.present();
}

fn present_up_to_date_dialog(
    parent: &gtk::Window,
    app_name: &str,
    current_version: &str,
    sender: &ComponentSender<UpdaterModel>,
) {
    let dialog = adw::MessageDialog::builder()
        .heading(&*crate::i18n::tr("updater.up_to_date_title"))
        .body(format!(
            "{} {}.\n{} v{} {}.",
            app_name,
            crate::i18n::tr("updater.up_to_date_body_1"),
            crate::i18n::tr("updater.up_to_date_body_2"),
            current_version,
            crate::i18n::tr("updater.up_to_date_body_3")
        ))
        .transient_for(parent)
        .build();

    dialog.add_response("ok", &crate::i18n::tr("updater.ok_btn"));
    dialog.set_default_response(Some("ok"));
    let sender = sender.clone();
    dialog.connect_response(None, move |dlg, _| {
        dlg.close();
        sender.input(UpdaterInput::DialogClosed);
    });
    dialog.present();
}

fn present_error_dialog(parent: &gtk::Window, error: &str, sender: &ComponentSender<UpdaterModel>) {
    let dialog = adw::MessageDialog::builder()
        .heading(&*crate::i18n::tr("updater.error_title"))
        .body(format!(
            "{}\n\nError: {}",
            crate::i18n::tr("updater.error_body"),
            error
        ))
        .transient_for(parent)
        .build();

    dialog.add_response("ok", &crate::i18n::tr("updater.ok_btn"));
    dialog.set_default_response(Some("ok"));
    let sender = sender.clone();
    dialog.connect_response(None, move |dlg, _| {
        dlg.close();
        sender.input(UpdaterInput::DialogClosed);
    });
    dialog.present();
}

fn present_success_dialog(parent: &gtk::Window, sender: &ComponentSender<UpdaterModel>) {
    let dialog = adw::MessageDialog::builder()
        .heading(&*crate::i18n::tr("updater.success_title"))
        .body(&*crate::i18n::tr("updater.success_body"))
        .transient_for(parent)
        .build();

    dialog.add_response(
        "restart",
        &crate::i18n::tr("updater.restart_btn"),
    );
    dialog.add_response(
        "later",
        &crate::i18n::tr("updater.later_btn"),
    );
    dialog.set_response_appearance("restart", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("restart"));

    let sender = sender.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response == "restart" {
            let _ = sender.output(UpdaterOutput::Restart);
        } else {
            dlg.close();
        }
        sender.input(UpdaterInput::DialogClosed);
    });
    dialog.present();
}
