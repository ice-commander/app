use gtk::prelude::*;
use gtk_fm_ui::{
    build_path_string, BreadcrumbSegment, FmPanelInit, FmPanelInput, FmPanelModel, FmPanelOutput,
    PathSegment, RemoteFileEntry, SourceInfo,
};
use relm4::prelude::*;

fn list_dir(parts: &[String]) -> Vec<RemoteFileEntry> {
    let path = build_path_string(parts);
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                Some(meta.permissions().mode())
            };
            #[cfg(not(unix))]
            let permissions = None;
            entries.push(RemoteFileEntry {
                name: e.file_name().to_string_lossy().to_string(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                modified,
                permissions,
            });
        }
    }
    entries
}

fn segments(parts: &[String]) -> Vec<PathSegment> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for p in parts {
        cur.push(p.clone());
        out.push(PathSegment {
            name: p.clone(),
            path: build_path_string(&cur),
        });
    }
    out
}

struct LocalFm {
    current: Vec<String>,
    fm: Controller<FmPanelModel>,
}

impl LocalFm {
    fn relist(&self) {
        let parts = &self.current;
        let _ = self.fm.sender().send(FmPanelInput::Listing {
            path: build_path_string(parts),
            entries: list_dir(parts),
            breadcrumb: BreadcrumbSegment::from_segments(&segments(parts)),
            source: SourceInfo {
                is_local: true,
                display_name: None,
                fs_label: None,
                root_icon: "/com/fm-ui/gtk/home.svg".to_string(),
                root_icon_svg: None,
                connection_id: None,
            },
            select_name: None,
        });
    }
}

#[derive(Debug)]
enum Msg {
    Fm(FmPanelOutput),
}

impl SimpleComponent for LocalFm {
    type Init = ();
    type Input = Msg;
    type Output = ();
    type Root = gtk::Window;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Window::builder()
            .title("gtk-fm-ui — local filesystem demo")
            .default_width(900)
            .default_height(600)
            .build()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let fm = FmPanelModel::builder()
            .launch(FmPanelInit::new(client_config::AppConfig::new("fm-demo")))
            .forward(sender.input_sender(), Msg::Fm);
        root.set_child(Some(fm.widget()));

        let model = LocalFm {
            current: Vec::new(),
            fm,
        };
        model.relist();
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        let Msg::Fm(out) = msg;
        match out {
            FmPanelOutput::NavigateEnter(name) => {
                self.current.push(name);
                self.relist();
            }
            FmPanelOutput::NavigateUp => {
                self.current.pop();
                self.relist();
            }
            FmPanelOutput::NavigateLevel(idx) => {
                self.current.truncate(idx);
                self.relist();
            }
            FmPanelOutput::Refresh => self.relist(),
            FmPanelOutput::Start | FmPanelOutput::Back => {
                self.current.clear();
                self.relist();
            }
            FmPanelOutput::Mkdir { parent, name } => {
                let target = std::path::Path::new(&parent).join(&name);
                match std::fs::create_dir(&target) {
                    Ok(()) => {
                        let _ = self
                            .fm
                            .sender()
                            .send(FmPanelInput::EntryAdded { name, is_dir: true });
                    }
                    Err(e) => {
                        let _ = self.fm.sender().send(FmPanelInput::OpFailed {
                            title: "Create Directory Failed".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            FmPanelOutput::Delete { paths, names } => {
                for p in &paths {
                    let path = std::path::Path::new(p);
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(path)
                    } else {
                        std::fs::remove_file(path)
                    };
                }
                let _ = self
                    .fm
                    .sender()
                    .send(FmPanelInput::EntriesRemoved { names });
            }
            FmPanelOutput::Rename { old_path, new_path } => {
                match std::fs::rename(&old_path, &new_path) {
                    Ok(()) => {
                        let old = std::path::Path::new(&old_path)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let new = std::path::Path::new(&new_path)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let _ = self
                            .fm
                            .sender()
                            .send(FmPanelInput::EntryRenamed { old, new });
                    }
                    Err(e) => {
                        let _ = self.fm.sender().send(FmPanelInput::OpFailed {
                            title: "Rename Failed".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            FmPanelOutput::StateChanged { .. } => {}
            other => {
                println!("[demo] unhandled panel intent: {:?}", other);
            }
        }
    }
}

fn main() {
    RelmApp::new("com.icecommander.fm.local_demo").run::<LocalFm>(());
}
