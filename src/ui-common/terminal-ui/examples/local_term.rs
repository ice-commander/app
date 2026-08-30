use gtk::prelude::*;
use gtk_terminal_ui::{TerminalInit, TerminalInput, TerminalModel, TerminalOutput};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use relm4::prelude::*;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

struct LocalTerm {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _terminal: Controller<TerminalModel>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[derive(Debug)]
enum Msg {
    Term(TerminalOutput),
}

impl SimpleComponent for LocalTerm {
    type Init = ();
    type Input = Msg;
    type Output = ();
    type Root = gtk::Window;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Window::builder()
            .title("gtk-terminal-ui — local shell demo")
            .default_width(820)
            .default_height(520)
            .build()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let child = pair
            .slave
            .spawn_command(CommandBuilder::new(shell))
            .expect("spawn shell");
        let mut reader = pair.master.try_clone_reader().expect("pty reader");
        let writer = Arc::new(Mutex::new(pair.master.take_writer().expect("pty writer")));

        let terminal = TerminalModel::builder()
            .launch(TerminalInit {
                show_toolbar: true,
                back_tooltip: Some("Local shell".to_string()),
                config: client_config::AppConfig::new("terminal-demo"),
            })
            .forward(sender.input_sender(), Msg::Term);
        root.set_child(Some(terminal.widget()));

        let feed = terminal.sender().clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if feed.send(TerminalInput::Feed(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let _ = terminal.sender().send(TerminalInput::GrabFocus);

        let model = LocalTerm {
            master: pair.master,
            writer,
            _terminal: terminal,
            _child: child,
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            Msg::Term(TerminalOutput::Input(data)) => {
                if let Ok(mut w) = self.writer.lock() {
                    let _ = w.write_all(&data);
                    let _ = w.flush();
                }
            }
            Msg::Term(TerminalOutput::Resize { rows, cols }) => {
                let _ = self.master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            Msg::Term(TerminalOutput::Restart) | Msg::Term(TerminalOutput::Back) => {}
        }
    }
}

fn main() {
    RelmApp::new("com.icecommander.terminal.local_demo").run::<LocalTerm>(());
}
