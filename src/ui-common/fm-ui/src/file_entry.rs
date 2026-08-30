use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;

mod imp {
    use super::*;
    use gtk::glib::subclass::prelude::*;

    #[derive(Default)]
    pub struct FileEntry {
        pub name: RefCell<String>,
        pub path: RefCell<String>,
        pub is_dir: RefCell<bool>,
        pub size: RefCell<u64>,
        pub date: RefCell<String>,
        pub permissions: RefCell<Option<u32>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileEntry {
        const NAME: &'static str = "FmFileEntry";
        type Type = super::FileEntry;
    }

    impl ObjectImpl for FileEntry {
        fn properties() -> &'static [glib::ParamSpec] {
            use std::sync::OnceLock;
            static PROPERTIES: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            PROPERTIES.get_or_init(|| {
                vec![
                    glib::ParamSpecString::builder("name").build(),
                    glib::ParamSpecString::builder("path").build(),
                    glib::ParamSpecBoolean::builder("is-dir").build(),
                    glib::ParamSpecUInt64::builder("size").build(),
                    glib::ParamSpecString::builder("date").build(),
                    glib::ParamSpecUInt::builder("permissions").build(),
                ]
            })
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "name" => {
                    let name = value.get().expect("type error");
                    self.name.replace(name);
                }
                "path" => {
                    let path = value.get().expect("type error");
                    self.path.replace(path);
                }
                "is-dir" => {
                    let is_dir = value.get().expect("type error");
                    self.is_dir.replace(is_dir);
                }
                "size" => {
                    let size = value.get().expect("type error");
                    self.size.replace(size);
                }
                "date" => {
                    let date = value.get().expect("type error");
                    self.date.replace(date);
                }
                "permissions" => {
                    let perms: u32 = value.get().expect("type error");
                    if perms == u32::MAX {
                        self.permissions.replace(None);
                    } else {
                        self.permissions.replace(Some(perms));
                    }
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "name" => self.name.borrow().to_value(),
                "path" => self.path.borrow().to_value(),
                "is-dir" => self.is_dir.borrow().to_value(),
                "size" => self.size.borrow().to_value(),
                "date" => self.date.borrow().to_value(),
                "permissions" => {
                    let perms = self.permissions.borrow().unwrap_or(u32::MAX);
                    perms.to_value()
                }
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub struct FileEntry(ObjectSubclass<imp::FileEntry>);
}

impl FileEntry {
    pub fn new(
        name: &str,
        path: &str,
        is_dir: bool,
        size: u64,
        date: &str,
        permissions: Option<u32>,
    ) -> Self {
        glib::Object::builder()
            .property("name", name)
            .property("path", path)
            .property("is-dir", is_dir)
            .property("size", size)
            .property("date", date)
            .property("permissions", permissions.unwrap_or(u32::MAX))
            .build()
    }

    pub fn name(&self) -> String {
        self.property("name")
    }

    pub fn path(&self) -> String {
        self.property("path")
    }

    pub fn is_dir(&self) -> bool {
        self.property("is-dir")
    }

    pub fn size(&self) -> u64 {
        self.property("size")
    }

    pub fn date(&self) -> String {
        self.property("date")
    }

    pub fn permissions(&self) -> Option<u32> {
        let val: u32 = self.property("permissions");
        if val == u32::MAX {
            None
        } else {
            Some(val)
        }
    }

    pub fn set_permissions(&self, perms: Option<u32>) {
        self.set_property("permissions", perms.unwrap_or(u32::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_survives_a_construction_round_trip() {
        let entry = FileEntry::new("notes.txt", "/home/ice/notes.txt", false, 4096, "2026-08-29", Some(0o644));
        assert_eq!(entry.name(), "notes.txt");
        assert_eq!(entry.path(), "/home/ice/notes.txt");
        assert!(!entry.is_dir());
        assert_eq!(entry.size(), 4096);
        assert_eq!(entry.date(), "2026-08-29");
        assert_eq!(entry.permissions(), Some(0o644));
    }

    #[test]
    fn missing_permissions_round_trip_as_none() {
        let entry = FileEntry::new("remote", "/remote", true, 0, "", None);
        assert_eq!(entry.permissions(), None);
    }

    #[test]
    fn set_permissions_overwrites_in_both_directions() {
        let entry = FileEntry::new("script.sh", "/script.sh", false, 12, "", Some(0o600));
        entry.set_permissions(Some(0o755));
        assert_eq!(entry.permissions(), Some(0o755));
        entry.set_permissions(None);
        assert_eq!(entry.permissions(), None);
    }

    #[test]
    fn zero_is_a_real_permission_mode_and_not_the_none_sentinel() {
        let entry = FileEntry::new("locked", "/locked", false, 1, "", Some(0));
        assert_eq!(entry.permissions(), Some(0));
    }

    #[test]
    fn unicode_names_and_huge_sizes_are_stored_verbatim() {
        let entry = FileEntry::new("отчёт 🎧.mp3", "/музыка/отчёт 🎧.mp3", false, u64::MAX, "", None);
        assert_eq!(entry.name(), "отчёт 🎧.mp3");
        assert_eq!(entry.path(), "/музыка/отчёт 🎧.mp3");
        assert_eq!(entry.size(), u64::MAX);
    }

    #[test]
    fn empty_strings_are_preserved_rather_than_defaulted() {
        let entry = FileEntry::new("", "", false, 0, "", None);
        assert_eq!(entry.name(), "");
        assert_eq!(entry.date(), "");
    }

    #[test]
    fn entries_are_independent_objects() {
        let a = FileEntry::new("a", "/a", true, 1, "d1", Some(0o700));
        let b = FileEntry::new("b", "/b", false, 2, "d2", None);
        a.set_permissions(None);
        assert_eq!(b.name(), "b");
        assert!(a.is_dir());
        assert!(!b.is_dir());
    }
}
