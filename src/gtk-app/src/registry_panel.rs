use gtk::glib;
use gtk_registry_ui::{RegistryInit, RegistryInput, RegistryModel, RegistryOutput};
use relm4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct RegistryPanel {
    pub container: gtk::Box,
    sender: relm4::Sender<RegistryInput>,
    back_cb: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    _controller: Rc<Controller<RegistryModel>>,
}

impl RegistryPanel {
    pub fn new() -> Self {
        let (out_tx, out_rx) = relm4::channel::<RegistryOutput>();
        let controller = RegistryModel::builder()
            .launch(RegistryInit { show_toolbar: true })
            .forward(&out_tx, |o| o);
        let container = controller.widget().clone();
        let sender = controller.sender().clone();
        let back_cb: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

        {
            let sender = sender.clone();
            let back_cb = back_cb.clone();
            glib::spawn_future_local(async move {
                while let Some(out) = out_rx.recv().await {
                    match out {
                        RegistryOutput::RequestKeys { path } => {
                            Self::answer_keys(&sender, path);
                        }
                        RegistryOutput::SetValue { path, value_name, data } => {
                            #[cfg(target_os = "windows")]
                            {
                                match ic_platform::registry::set_registry_value(
                                    &path,
                                    &value_name,
                                    &data,
                                ) {
                                    Ok(_) => Self::answer_keys(&sender, path),
                                    Err(e) => println!(
                                        "[APP LOG] ❌ Failed to update registry value: {}",
                                        e
                                    ),
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            let _ = (path, value_name, data);
                        }
                        RegistryOutput::DeleteEntry { path, is_key, value_name } => {
                            #[cfg(target_os = "windows")]
                            {
                                match ic_platform::registry::delete_registry_entry(
                                    &path,
                                    is_key,
                                    value_name.clone(),
                                ) {
                                    Ok(_) => Self::answer_keys(&sender, path),
                                    Err(e) => println!(
                                        "[APP LOG] ❌ Failed to delete registry entry: {}",
                                        e
                                    ),
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            let _ = (path, is_key, value_name);
                        }
                        RegistryOutput::Back => {
                            if let Some(cb) = back_cb.borrow().as_ref() {
                                cb();
                            }
                        }
                    }
                }
            });
        }

        Self {
            container,
            sender,
            back_cb,
            _controller: Rc::new(controller),
        }
    }

    fn answer_keys(sender: &relm4::Sender<RegistryInput>, path: String) {
        #[cfg(target_os = "windows")]
        {
            let (subkeys, values, error) =
                ic_platform::registry::read_registry_keys(&path);
            if let Some(err) = error {
                println!("[APP LOG] ❌ Registry access error: {}", err);
            } else {
                let _ = sender.send(RegistryInput::Entries { path, subkeys, values });
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (sender, path);
            println!(
                "[APP LOG] Registry Editor is only available when the local node is running on Windows"
            );
        }
    }

    pub fn set_back_callback(&self, cb: impl Fn() + 'static) {
        *self.back_cb.borrow_mut() = Some(Box::new(cb));
    }

    pub fn open_local(&self) {
        let _ = self.sender.send(RegistryInput::Reset);
    }
}

impl Default for RegistryPanel {
    fn default() -> Self {
        Self::new()
    }
}
