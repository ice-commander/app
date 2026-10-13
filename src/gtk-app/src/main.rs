#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::rc::Rc;

use gtk::glib;
use ice_commander_gtk::Desktop;

#[tokio::main]
async fn main() -> glib::ExitCode {
    ice_commander_gtk::run(Rc::new(Desktop::from_env()))
}
