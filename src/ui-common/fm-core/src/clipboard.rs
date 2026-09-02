use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::rpc::FileSystemRpc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipKind {
    Copy,
    Cut,
}

#[derive(Clone)]
pub struct ClipItem {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
}

pub struct Clip {
    pub kind: ClipKind,
    pub side: String,
    pub source_path: String,
    pub items: Vec<ClipItem>,
    pub source: Rc<dyn FileSystemRpc>,
    pub anchor: Weak<dyn FileSystemRpc>,
    pub rebuildable: bool,
    #[allow(clippy::type_complexity)]
    pub refresh_source: Option<Rc<dyn Fn()>>,
}

impl Clip {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadgeState {
    pub cut_badge: bool,
    pub copy_badge: bool,
    pub paste_enabled: bool,
    pub clear_visible: bool,
}

pub fn badge_state(count: usize, mine: bool, cut: bool) -> BadgeState {
    let any = count > 0;
    BadgeState {
        cut_badge: any && mine && cut,
        copy_badge: any && mine && !cut,
        paste_enabled: any,
        clear_visible: any,
    }
}

#[derive(Default)]
pub struct Clipboard {
    slot: RefCell<Option<Clip>>,
    #[allow(clippy::type_complexity)]
    on_change: RefCell<Vec<Rc<dyn Fn(usize, &str, bool)>>>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, clip: Clip) {
        let (n, side, cut) = (clip.len(), clip.side.clone(), clip.kind == ClipKind::Cut);
        *self.slot.borrow_mut() = Some(clip);
        self.notify(n, &side, cut);
    }

    pub fn clear(&self) {
        if self.slot.borrow().is_none() {
            return;
        }
        *self.slot.borrow_mut() = None;
        self.notify(0, "", false);
    }

    pub fn count(&self) -> usize {
        self.slot.borrow().as_ref().map_or(0, Clip::len)
    }

    pub fn kind(&self) -> Option<ClipKind> {
        self.slot.borrow().as_ref().map(|c| c.kind)
    }

    pub fn with<T>(&self, f: impl FnOnce(&Clip) -> T) -> Option<T> {
        self.slot.borrow().as_ref().map(f)
    }

    pub fn connect_changed(&self, f: Rc<dyn Fn(usize, &str, bool)>) {
        self.on_change.borrow_mut().push(f);
    }

    pub fn drop_if_unreachable(&self, live: &[Rc<dyn FileSystemRpc>]) {
        let gone = match &*self.slot.borrow() {
            None => return,
            Some(c) if c.rebuildable => false,
            Some(c) => match c.anchor.upgrade() {
                None => true,
                Some(src) => !live.iter().any(|f| Rc::ptr_eq(f, &src)),
            },
        };
        if gone {
            self.clear();
        }
    }

    fn notify(&self, n: usize, side: &str, cut: bool) {
        let hooks = self.on_change.borrow().clone();
        for h in hooks {
            h(n, side, cut);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRpc;

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for TestRpc {}

    fn fs() -> Rc<dyn FileSystemRpc> {
        Rc::new(TestRpc)
    }

    fn clip_with(
        rebuildable: bool,
        source: Rc<dyn FileSystemRpc>,
        anchor: &Rc<dyn FileSystemRpc>,
    ) -> Clip {
        Clip {
            kind: ClipKind::Copy,
            side: "left".to_string(),
            source_path: "/tmp".to_string(),
            items: vec![ClipItem {
                name: "a.txt".to_string(),
                is_dir: false,
                size: 1,
                permissions: None,
            }],
            source,
            anchor: Rc::downgrade(anchor),
            rebuildable,
            refresh_source: None,
        }
    }

    #[test]
    fn count_follows_the_slot() {
        let cb = Clipboard::new();
        assert_eq!(cb.count(), 0);
        assert!(cb.kind().is_none());
    }

    #[test]
    fn clearing_an_empty_clipboard_does_not_notify() {
        let cb = Clipboard::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, _, _| s.borrow_mut().push(n)));
        cb.clear();
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn a_clip_outlives_the_snapshot_it_was_taken_through() {
        let level = fs();
        let cb = Clipboard::new();
        cb.set(clip_with(false, fs(), &level));
        cb.drop_if_unreachable(&[level.clone()]);
        assert_eq!(cb.count(), 1);
        assert!(cb.with(|c| Rc::strong_count(&c.source)).unwrap() >= 1);
    }

    #[test]
    fn a_clip_dies_with_its_level_though_the_snapshot_still_lives() {
        let level = fs();
        let cb = Clipboard::new();
        cb.set(clip_with(false, fs(), &level));
        drop(level);
        cb.drop_if_unreachable(&[]);
        assert_eq!(cb.count(), 0);
    }

    #[test]
    fn a_clip_dies_when_its_level_is_no_longer_open_anywhere() {
        let level = fs();
        let cb = Clipboard::new();
        cb.set(clip_with(false, fs(), &level));
        cb.drop_if_unreachable(&[fs()]);
        assert_eq!(cb.count(), 0);
    }

    #[test]
    fn a_rebuildable_clip_survives_losing_its_level() {
        let level = fs();
        let cb = Clipboard::new();
        cb.set(clip_with(true, fs(), &level));
        drop(level);
        cb.drop_if_unreachable(&[]);
        assert_eq!(cb.count(), 1);
    }

    #[test]
    fn an_empty_clipboard_shows_nothing_anywhere() {
        for mine in [false, true] {
            for cut in [false, true] {
                let st = badge_state(0, mine, cut);
                assert_eq!(
                    st,
                    BadgeState {
                        cut_badge: false,
                        copy_badge: false,
                        paste_enabled: false,
                        clear_visible: false
                    },
                    "mine={mine} cut={cut}"
                );
            }
        }
    }

    #[test]
    fn a_cut_badges_only_the_panel_it_came_from() {
        let here = badge_state(3, true, true);
        assert!(here.cut_badge && !here.copy_badge);
        let there = badge_state(3, false, true);
        assert!(!there.cut_badge && !there.copy_badge);
    }

    #[test]
    fn a_copy_badges_only_the_panel_it_came_from() {
        let here = badge_state(2, true, false);
        assert!(here.copy_badge && !here.cut_badge);
        let there = badge_state(2, false, false);
        assert!(!there.copy_badge && !there.cut_badge);
    }

    #[test]
    fn cut_and_copy_badges_are_never_both_lit() {
        for count in [0usize, 1, 9] {
            for mine in [false, true] {
                for cut in [false, true] {
                    let st = badge_state(count, mine, cut);
                    assert!(!(st.cut_badge && st.copy_badge), "count={count} mine={mine} cut={cut}");
                }
            }
        }
    }

    #[test]
    fn paste_and_clear_are_offered_on_both_panels() {
        for mine in [false, true] {
            let st = badge_state(1, mine, true);
            assert!(st.paste_enabled, "paste must be offered on the other panel too");
            assert!(st.clear_visible);
        }
    }

    #[test]
    fn taking_a_clip_notifies_the_count_side_and_kind() {
        let cb = Clipboard::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, side, cut| {
            s.borrow_mut().push((n, side.to_string(), cut))
        }));
        let level = fs();
        let mut c = clip_with(false, fs(), &level);
        c.side = "right".to_string();
        c.kind = ClipKind::Cut;
        cb.set(c);
        assert_eq!(seen.borrow().as_slice(), &[(1, "right".to_string(), true)]);
    }

    #[test]
    fn a_second_take_replaces_the_first_and_renotifies() {
        let cb = Clipboard::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, side, cut| {
            s.borrow_mut().push((n, side.to_string(), cut))
        }));
        let level = fs();
        cb.set(clip_with(false, fs(), &level));
        let mut second = clip_with(false, fs(), &level);
        second.side = "right".to_string();
        second.kind = ClipKind::Cut;
        second.items.push(ClipItem {
            name: "b.txt".to_string(),
            is_dir: false,
            size: 2,
            permissions: None,
        });
        cb.set(second);
        assert_eq!(cb.count(), 2);
        assert_eq!(cb.kind(), Some(ClipKind::Cut));
        assert_eq!(
            seen.borrow().as_slice(),
            &[(1, "left".to_string(), false), (2, "right".to_string(), true)]
        );
    }

    #[test]
    fn clearing_a_filled_clipboard_notifies_zero_and_no_side() {
        let cb = Clipboard::new();
        let level = fs();
        cb.set(clip_with(false, fs(), &level));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, side, cut| {
            s.borrow_mut().push((n, side.to_string(), cut))
        }));
        cb.clear();
        assert_eq!(seen.borrow().as_slice(), &[(0, String::new(), false)]);
        assert_eq!(cb.count(), 0);
        assert!(cb.kind().is_none());
    }

    #[test]
    fn dropping_an_unreachable_clip_notifies_so_the_badge_goes_out() {
        let cb = Clipboard::new();
        let level = fs();
        cb.set(clip_with(false, fs(), &level));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, _, _| s.borrow_mut().push(n)));
        drop(level);
        cb.drop_if_unreachable(&[]);
        assert_eq!(seen.borrow().as_slice(), &[0]);
    }

    #[test]
    fn a_reachable_clip_does_not_notify_on_navigation() {
        let cb = Clipboard::new();
        let level = fs();
        cb.set(clip_with(false, fs(), &level));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        cb.connect_changed(Rc::new(move |n, _, _| s.borrow_mut().push(n)));
        cb.drop_if_unreachable(&[level.clone()]);
        assert!(seen.borrow().is_empty(), "a still-open level must not flicker the badge");
    }
}
