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
        let Some(row) = self.panes[self.active].selected_row() else { return };
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
        let Some(row) = self.panes[self.active].selected_row() else { return };
        if row.is_parent {
            return;
        }
        let rel = join_rel(&self.panes[self.active].active_relative(), &row.name);
        self.overlay = Overlay::Confirm {
            title: "Delete".into(),
            message: format!("Delete \"{}\"?", row.name),
            action: ConfirmAction::Delete { rel },
        };
    }

    pub(crate) fn prompt_transfer(&mut self, move_it: bool) {
        let Some(row) = self.panes[self.active].selected_row() else { return };
        if row.is_parent {
            return;
        }
        if row.is_dir {
            self.set_message("Not supported yet", "Directory copy/move is not implemented in v1 (files only).");
            return;
        }
        let other = self.active ^ 1;
        let src_rel = join_rel(&self.panes[self.active].active_relative(), &row.name);
        let dst_dir = self.panes[other].active_relative();
        let dst_rel = join_rel(&dst_dir, &row.name);
        let dst_abs = self.panes[other].core.path.borrow().absolute_path();
        let verb = if move_it { "Move" } else { "Copy" };
        self.overlay = Overlay::Confirm {
            title: verb.into(),
            message: format!("{verb} \"{}\" to {dst_abs}?", row.name),
            action: ConfirmAction::Transfer { move_it, src_rel, dst_rel },
        };
    }

    pub(crate) async fn do_mkdir(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let core = self.panes[self.active].core.clone();
        let base = self.panes[self.active].active_relative();
        match core.active_provider().create_directory(base, name, None).await {
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

    pub(crate) async fn do_delete(&mut self, rel: String) {
        let core = self.panes[self.active].core.clone();
        match core.active_provider().delete_entries(vec![rel]).await {
            Ok(_) => {
                let _ = core.refresh().await;
            }
            Err(e) => self.set_message("Delete failed", e.to_string()),
        }
    }

    pub(crate) async fn do_transfer(&mut self, move_it: bool, src_rel: String, dst_rel: String) {
        let other = self.active ^ 1;
        let src_core = self.panes[self.active].core.clone();
        let dst_core = self.panes[other].core.clone();
        let src_provider = src_core.active_provider();
        let dst_provider = dst_core.active_provider();
        let data = match src_provider.read_file(src_rel.clone(), None).await {
            Ok(d) => d,
            Err(e) => return self.set_message("Read failed", e.to_string()),
        };
        if let Err(e) = dst_provider.write_file(dst_rel, data, None, None).await {
            return self.set_message("Write failed", e.to_string());
        }
        if move_it {
            let _ = src_provider.delete_entries(vec![src_rel]).await;
            let _ = src_core.refresh().await;
        }
        let _ = dst_core.refresh().await;
    }
}
