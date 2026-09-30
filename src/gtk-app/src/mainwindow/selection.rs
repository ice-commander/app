use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::ActivePanelSide;
use crate::plugin_host::SelectedItem;

pub type PanelSelection = Rc<dyn Fn() -> Vec<SelectedItem>>;

thread_local! {
    static ACTING: RefCell<Option<PanelSelection>> = const { RefCell::new(None) };
}

/// A plugin button acts on the panel it sits on, even while the other panel holds the focus.
pub fn while_acting_for<T>(panel: PanelSelection, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<PanelSelection>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTING.with(|a| *a.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(ACTING.with(|a| a.borrow_mut().replace(panel)));
    f()
}

pub fn install(active: Rc<Cell<ActivePanelSide>>, left: PanelSelection, right: PanelSelection) {
    crate::plugin_host::set_selection_provider(Rc::new(move || {
        if let Some(panel) = ACTING.with(|a| a.borrow().clone()) {
            return panel();
        }
        match active.get() {
            ActivePanelSide::Left => left(),
            ActivePanelSide::Right => right(),
            ActivePanelSide::None => Vec::new(),
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(name: &Rc<RefCell<String>>) -> PanelSelection {
        let name = name.clone();
        Rc::new(move || {
            vec![SelectedItem {
                path: name.borrow().clone(),
                key: name.borrow().clone(),
                is_dir: false,
            }]
        })
    }

    fn what_a_plugin_sees() -> Vec<String> {
        let selection = (crate::plugin_host::host_ref().selection)();
        selection
            .as_slice()
            .iter()
            .filter_map(|i| i.path_string())
            .collect()
    }

    #[test]
    fn the_selection_comes_from_the_active_panel_not_from_the_tab_opened_last() {
        let active = Rc::new(Cell::new(ActivePanelSide::Left));
        let left = Rc::new(RefCell::new("left-tab-1".to_string()));
        let right = Rc::new(RefCell::new("right-tab-1".to_string()));
        install(active.clone(), tab(&left), tab(&right));

        *right.borrow_mut() = "right-tab-2".to_string();
        assert_eq!(what_a_plugin_sees(), ["left-tab-1"]);

        active.set(ActivePanelSide::Right);
        assert_eq!(what_a_plugin_sees(), ["right-tab-2"]);

        active.set(ActivePanelSide::Left);
        *left.borrow_mut() = "left-tab-2".to_string();
        assert_eq!(what_a_plugin_sees(), ["left-tab-2"]);

        active.set(ActivePanelSide::None);
        assert!(what_a_plugin_sees().is_empty());
    }

    #[test]
    fn a_button_pressed_on_the_inactive_panel_is_answered_by_that_panel() {
        let active = Rc::new(Cell::new(ActivePanelSide::Left));
        let left = Rc::new(RefCell::new("left".to_string()));
        let right = Rc::new(RefCell::new("right".to_string()));
        install(active, tab(&left), tab(&right));

        let during = while_acting_for(tab(&right), what_a_plugin_sees);
        assert_eq!(during, ["right"]);
        assert_eq!(what_a_plugin_sees(), ["left"]);
    }
}
