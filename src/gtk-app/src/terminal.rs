#![allow(deprecated)]
use gtk::glib;
use gtk::prelude::*;
use gtk_terminal_ui::{TerminalInit, TerminalInput, TerminalModel, TerminalOutput};
use ic_platform::terminal::{spawn_pty_command, PtySession};
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::mpsc as tokio_mpsc;

#[derive(Clone)]
pub struct TerminalView {
    pub container: gtk::Box,
    sender: relm4::Sender<TerminalInput>,
    _controller: Rc<Controller<TerminalModel>>,
    pub pty_input_tx: Rc<RefCell<Option<tokio_mpsc::Sender<Vec<u8>>>>>,
    pub pty_resize_tx: Rc<RefCell<Option<tokio_mpsc::Sender<(u16, u16)>>>>,
    #[allow(dead_code)]
    pub output_tx: broadcast::Sender<Vec<u8>>,
    pub on_visibility_changed: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>>,
    pub on_session_ended: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    pub session_gen: Rc<Cell<u64>>,
    start: Rc<dyn Fn()>,
    args: Rc<RefCell<Vec<String>>>,
    cwd: Rc<RefCell<Option<String>>>,
    session_factory: SessionFactorySlot,
}

type SessionFactory = Rc<dyn Fn() -> Result<PtySession, String>>;
type SessionFactorySlot = Rc<RefCell<Option<SessionFactory>>>;

impl TerminalView {
    pub fn new(config: client_config::AppConfig, output_tx: broadcast::Sender<Vec<u8>>) -> Self {
        let (out_tx, out_rx) = relm4::channel::<TerminalOutput>();
        let controller = TerminalModel::builder()
            .launch(TerminalInit {
                show_toolbar: false,
                back_tooltip: None,
                config,
            })
            .forward(&out_tx, |o| o);
        let container = controller.widget().clone();
        let sender = controller.sender().clone();

        let pty_input_tx: Rc<RefCell<Option<tokio_mpsc::Sender<Vec<u8>>>>> =
            Rc::new(RefCell::new(None));
        let pty_resize_tx: Rc<RefCell<Option<tokio_mpsc::Sender<(u16, u16)>>>> =
            Rc::new(RefCell::new(None));
        let on_session_ended: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let session_gen = Rc::new(Cell::new(0u64));
        let args = Rc::new(RefCell::new(Vec::new()));
        let cwd = Rc::new(RefCell::new(None));
        let session_factory: SessionFactorySlot = Rc::new(RefCell::new(None));

        let start: Rc<dyn Fn()> = {
            let sender = sender.clone();
            let pty_input_tx = pty_input_tx.clone();
            let pty_resize_tx = pty_resize_tx.clone();
            let output_tx = output_tx.clone();
            let on_session_ended = on_session_ended.clone();
            let session_gen = session_gen.clone();
            let args = args.clone();
            let cwd = cwd.clone();
            let session_factory = session_factory.clone();
            Rc::new(move || {
                *pty_input_tx.borrow_mut() = None;
                let _ = sender.send(TerminalInput::Clear);
                session_gen.set(session_gen.get().wrapping_add(1));
                if output_tx.receiver_count() > 0 {
                    let _ = output_tx.send(b"\x1b[2J\x1b[H".to_vec());
                }

                let factory = session_factory.borrow().clone();
                let started = match factory {
                    Some(make) => make(),
                    None => spawn_pty_command(args.borrow().clone(), cwd.borrow().clone()),
                };
                match started {
                    Ok(session) => {
                        *pty_input_tx.borrow_mut() = Some(session.input_tx.clone());
                        *pty_resize_tx.borrow_mut() = Some(session.resize_tx.clone());

                        let (gui_tx, gui_rx) = std::sync::mpsc::channel::<Vec<u8>>();
                        let sender_feed = sender.clone();
                        let on_ended = on_session_ended.clone();
                        glib::timeout_add_local(Duration::from_millis(20), move || {
                            while let Ok(data) = gui_rx.try_recv() {
                                if data.is_empty() {
                                    let _ = sender_feed
                                        .send(TerminalInput::Feed(b"\r\n[Session Ended]\r\n".to_vec()));
                                    if let Some(cb) = on_ended.borrow().as_ref() {
                                        cb();
                                    }
                                    return glib::ControlFlow::Break;
                                } else {
                                    let _ = sender_feed.send(TerminalInput::Feed(data));
                                }
                            }
                            glib::ControlFlow::Continue
                        });

                        let mut rx = session.output_rx;
                        let output_tx = output_tx.clone();
                        tokio::spawn(async move {
                            while let Some(data) = rx.recv().await {
                                if output_tx.receiver_count() > 0 {
                                    let _ = output_tx.send(data.clone());
                                }
                                if gui_tx.send(data).is_err() {
                                    break;
                                }
                            }
                            let _ = gui_tx.send(Vec::new());
                            if output_tx.receiver_count() > 0 {
                                let _ = output_tx.send(b"\r\n[Session Ended]\r\n".to_vec());
                            }
                        });
                    }
                    Err(e) => {
                        let msg = format!("Failed to start terminal: {e}\r\n");
                        let _ = sender.send(TerminalInput::Feed(msg.clone().into_bytes()));
                        if output_tx.receiver_count() > 0 {
                            let _ = output_tx.send(msg.into_bytes());
                        }
                    }
                }
            })
        };

        {
            let pty_input_tx = pty_input_tx.clone();
            let pty_resize_tx = pty_resize_tx.clone();
            let start = start.clone();
            glib::spawn_future_local(async move {
                while let Some(out) = out_rx.recv().await {
                    match out {
                        TerminalOutput::Input(data) => {
                            if let Some(ref tx) = *pty_input_tx.borrow() {
                                let _ = tx.try_send(data);
                            }
                        }
                        TerminalOutput::Resize { rows, cols } => {
                            if let Some(ref tx) = *pty_resize_tx.borrow() {
                                let _ = tx.try_send((rows, cols));
                            }
                        }
                        TerminalOutput::Restart => start(),
                        TerminalOutput::Back => {}
                    }
                }
            });
        }

        Self {
            container,
            sender,
            _controller: Rc::new(controller),
            pty_input_tx,
            pty_resize_tx,
            output_tx,
            on_visibility_changed: Rc::new(RefCell::new(None)),
            on_session_ended,
            session_gen,
            start,
            args,
            cwd,
            session_factory,
        }
    }

    pub fn set_session_ended_callback(&self, cb: impl Fn() + 'static) {
        *self.on_session_ended.borrow_mut() = Some(Rc::new(cb));
    }

    pub fn notify_visibility(&self, open: bool) {
        if let Some(cb) = self.on_visibility_changed.borrow().as_ref() {
            cb(open);
        }
    }

    pub fn has_focus(&self) -> bool {
        self.container
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    pub fn start_command_session(&self, args: Vec<String>, cwd: Option<String>) {
        *self.session_factory.borrow_mut() = None;
        *self.args.borrow_mut() = args;
        *self.cwd.borrow_mut() = cwd;
        (self.start)();
        let _ = self.sender.send(TerminalInput::GrabFocus);
    }

    pub fn start_local_session(&self, cwd: Option<String>) {
        #[cfg(target_os = "windows")]
        {
            let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
            self.start_command_session(vec![shell], cwd);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
            let mut argv = vec![shell];
            if cfg!(target_os = "macos") {
                argv.push("-l".to_string());
            }
            self.start_command_session(argv, cwd);
        }
    }

    pub fn start_ssh_session(&self, target: fm_core::rpc::SshShellTarget) {
        *self.session_factory.borrow_mut() = Some(Rc::new(move || {
            Ok(virtualfs::ssh_shell::open_ssh_shell(target.clone(), 24, 80))
        }));
        (self.start)();
        let _ = self.sender.send(TerminalInput::GrabFocus);
    }

    pub fn stop_session(&self) {
        *self.pty_input_tx.borrow_mut() = None;
        *self.pty_resize_tx.borrow_mut() = None;
    }
}
