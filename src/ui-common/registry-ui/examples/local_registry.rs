use ic_model::RegistryValueInfo;
use gtk::prelude::*;
use gtk_registry_ui::{RegistryInit, RegistryInput, RegistryModel, RegistryOutput};
use relm4::prelude::*;

#[cfg(not(target_os = "windows"))]
use ic_model::RegistryValueData;

fn read_keys(path: &str) -> Option<(Vec<String>, Vec<RegistryValueInfo>)> {
    #[cfg(target_os = "windows")]
    {
        let (subkeys, values, error) = ic_platform::registry::read_registry_keys(path);
        if let Some(err) = error {
            println!("Error reading registry: {}", err);
            return None;
        }
        Some((subkeys, values))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut subkeys = Vec::new();
        let mut values = Vec::new();

        let normalized_path = path.replace("\\", "/");
        if normalized_path == "/" {
            subkeys = vec![
                "HKEY_CLASSES_ROOT".to_string(),
                "HKEY_CURRENT_USER".to_string(),
                "HKEY_LOCAL_MACHINE".to_string(),
                "HKEY_USERS".to_string(),
                "HKEY_CURRENT_CONFIG".to_string(),
            ];
        } else if normalized_path.starts_with("/HKEY_CURRENT_USER") {
            if normalized_path == "/HKEY_CURRENT_USER" {
                subkeys = vec![
                    "Software".to_string(),
                    "Control Panel".to_string(),
                    "Environment".to_string(),
                    "Keyboard Layout".to_string(),
                ];
                values = vec![RegistryValueInfo {
                    name: "DefaultVal".to_string(),
                    data: RegistryValueData::String("Hello From Linux Mock Registry!".to_string()),
                }];
            } else if normalized_path == "/HKEY_CURRENT_USER/Software" {
                subkeys = vec![
                    "IceCommander".to_string(),
                    "Google".to_string(),
                    "Microsoft".to_string(),
                ];
            } else if normalized_path == "/HKEY_CURRENT_USER/Software/IceCommander" {
                values = vec![
                    RegistryValueInfo {
                        name: "Theme".to_string(),
                        data: RegistryValueData::String("Dark".to_string()),
                    },
                    RegistryValueInfo {
                        name: "ShowHiddenFiles".to_string(),
                        data: RegistryValueData::DWord(1),
                    },
                    RegistryValueInfo {
                        name: "Volume".to_string(),
                        data: RegistryValueData::QWord(80),
                    },
                    RegistryValueInfo {
                        name: "BinaryData".to_string(),
                        data: RegistryValueData::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF]),
                    },
                ];
            } else {
                subkeys = vec!["SubKeyA".to_string(), "SubKeyB".to_string()];
                values = vec![RegistryValueInfo {
                    name: "MockString".to_string(),
                    data: RegistryValueData::String("Generic Mock".to_string()),
                }];
            }
        } else {
            subkeys = vec!["Folder1".to_string(), "Folder2".to_string()];
            values = vec![RegistryValueInfo {
                name: "MockValue".to_string(),
                data: RegistryValueData::DWord(12345),
            }];
        }

        Some((subkeys, values))
    }
}

struct LocalRegistry {
    window: gtk::Window,
    registry: Controller<RegistryModel>,
}

impl LocalRegistry {
    fn answer_keys(&self, path: String) {
        if let Some((subkeys, values)) = read_keys(&path) {
            let _ = self.registry.sender().send(RegistryInput::Entries {
                path,
                subkeys,
                values,
            });
        }
    }
}

#[derive(Debug)]
enum Msg {
    Registry(RegistryOutput),
}

impl SimpleComponent for LocalRegistry {
    type Init = ();
    type Input = Msg;
    type Output = ();
    type Root = gtk::Window;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Window::builder()
            .title("Local Registry Editor Test App")
            .default_width(800)
            .default_height(600)
            .build()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        gtk_registry_ui::init_resources();

        let registry = RegistryModel::builder()
            .launch(RegistryInit::default())
            .forward(sender.input_sender(), Msg::Registry);
        root.set_child(Some(registry.widget()));

        let _ = registry.sender().send(RegistryInput::Reset);

        let model = LocalRegistry {
            window: root,
            registry,
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            Msg::Registry(RegistryOutput::RequestKeys { path }) => {
                self.answer_keys(path);
            }
            Msg::Registry(RegistryOutput::SetValue {
                path,
                value_name,
                data,
            }) => {
                println!(
                    "set_registry_value path={}, name={}, data={:?}",
                    path, value_name, data
                );
                #[cfg(target_os = "windows")]
                {
                    let _result =
                        ic_platform::registry::set_registry_value(&path, &value_name, &data);
                }
                self.answer_keys(path);
            }
            Msg::Registry(RegistryOutput::DeleteEntry {
                path,
                is_key,
                value_name,
            }) => {
                println!(
                    "delete_registry_entry path={}, is_key={}, name={:?}",
                    path, is_key, value_name
                );
                #[cfg(target_os = "windows")]
                {
                    let _result =
                        ic_platform::registry::delete_registry_entry(&path, is_key, value_name);
                }
                self.answer_keys(path);
            }
            Msg::Registry(RegistryOutput::Back) => {
                self.window.close();
            }
        }
    }
}

fn main() {
    RelmApp::new("com.icecommander.localregistry").run::<LocalRegistry>(());
}
