use crate::app::App;
use crate::overlay::{ConfirmAction, InputAction, Overlay};
use crate::util::join_rel;

impl App {
    pub(crate) fn prompt_mkdir(&mut self) {
        self.overlay = Overlay::Input {
            title: "Create directory".into(),
            value: String::new(),
            action: InputAction::MkDir,
        };
    }

    pub(crate) fn prompt_rename(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else {
            return;
        };
        if row.is_parent {
            return;
        }
        let dir = self.panes[self.active].active_relative();
        let old_rel = join_rel(&dir, &row.name);
        self.overlay = Overlay::Input {
            title: "Rename".into(),
            value: row.name,
            action: InputAction::Rename { old_rel, dir },
        };
    }

    pub(crate) fn prompt_delete(&mut self) {
        let chosen = self.panes[self.active].chosen();
        if chosen.is_empty() {
            return;
        }
        let dir = self.panes[self.active].active_relative();
        let rels: Vec<String> = chosen.iter().map(|r| join_rel(&dir, &r.name)).collect();
        let message = match chosen.as_slice() {
            [one] => format!("Delete \"{}\"?", one.name),
            many => format!("Delete {} marked items?", many.len()),
        };
        self.overlay = Overlay::Confirm {
            title: "Delete".into(),
            message,
            action: ConfirmAction::Delete { rels },
        };
    }

    pub(crate) fn prompt_transfer(&mut self, move_it: bool) {
        let chosen = self.panes[self.active].chosen();
        if chosen.is_empty() {
            return;
        }
        if chosen.iter().any(|r| r.is_dir) {
            self.set_message(
                "Not supported yet",
                "Directory copy/move is not implemented in v1 (files only).",
            );
            return;
        }
        let other = self.active ^ 1;
        let src_dir = self.panes[self.active].active_relative();
        let dst_dir = self.panes[other].active_relative();
        let items: Vec<(String, String)> = chosen
            .iter()
            .map(|r| (join_rel(&src_dir, &r.name), join_rel(&dst_dir, &r.name)))
            .collect();
        let dst_abs = self.panes[other].core.path.borrow().absolute_path();
        let verb = if move_it { "Move" } else { "Copy" };
        let message = match chosen.as_slice() {
            [one] => format!("{verb} \"{}\" to {dst_abs}?", one.name),
            many => format!("{verb} {} marked items to {dst_abs}?", many.len()),
        };
        self.overlay = Overlay::Confirm {
            title: verb.into(),
            message,
            action: ConfirmAction::Transfer { move_it, items },
        };
    }

    pub(crate) async fn do_mkdir(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let core = self.panes[self.active].core.clone();
        let base = self.panes[self.active].active_relative();
        match core
            .active_provider()
            .create_directory(base, name, None)
            .await
        {
            Ok(_) => {
                let _ = core.refresh().await;
            }
            Err(e) => self.set_message("Create directory failed", e.to_string()),
        }
    }

    pub(crate) async fn do_rename(&mut self, old_rel: String, dir: String, new_name: String) {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let core = self.panes[self.active].core.clone();
        let new_rel = join_rel(&dir, &new_name);
        match core.active_provider().rename_entry(old_rel, new_rel).await {
            Ok(_) => {
                let _ = core.refresh().await;
                self.active_pane().select_name(&new_name);
            }
            Err(e) => self.set_message("Rename failed", e.to_string()),
        }
    }

    pub(crate) async fn do_delete(&mut self, rels: Vec<String>) {
        let core = self.panes[self.active].core.clone();
        match core.active_provider().delete_entries(rels).await {
            Ok(_) => {
                let _ = core.refresh().await;
            }
            Err(e) => self.set_message("Delete failed", e.to_string()),
        }
    }

    pub(crate) async fn do_transfer(&mut self, move_it: bool, items: Vec<(String, String)>) {
        let other = self.active ^ 1;
        let src_core = self.panes[self.active].core.clone();
        let dst_core = self.panes[other].core.clone();
        let src_provider = src_core.active_provider();
        let dst_provider = dst_core.active_provider();
        let mut moved: Vec<String> = Vec::new();
        let mut trouble = None;
        for (src_rel, dst_rel) in items {
            let data = match src_provider.read_file(src_rel.clone(), None).await {
                Ok(d) => d,
                Err(e) => {
                    trouble = Some(("Read failed", e.to_string()));
                    break;
                }
            };
            if let Err(e) = dst_provider.write_file(dst_rel, data, None, None).await {
                trouble = Some(("Write failed", e.to_string()));
                break;
            }
            moved.push(src_rel);
        }
        if move_it && !moved.is_empty() {
            let _ = src_provider.delete_entries(moved).await;
            let _ = src_core.refresh().await;
        }
        let _ = dst_core.refresh().await;
        if let Some((what, why)) = trouble {
            self.set_message(what, why);
        }
    }
}
