mod app;
mod editor;
mod fileops;
mod footer;
mod overlay;
mod pane;
mod sources;
mod term;
mod util;
mod viewer;

use std::io::{self, Stdout};
use std::rc::Rc;

use fm_core::rpc::FileSystemRpc;
use panel_core::RouterState;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEvent, KeyEventKind,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::app::{ui, App};
use crate::pane::Pane;
use crate::term::{term_recv, Focus};

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()
}

fn set_start_path(core: &RouterState, path: &str) {
    if path == "/" || path.is_empty() {
        return;
    }
    let base = core.local_provider.clone();
    let segs = panel_core::parse_path_to_segments(path);
    let levels = panel_core::nav::build_levels(&segs, base.clone());
    *core.path.borrow_mut() = panel_core::nav::NavPath::from_levels(levels, base);
}

pub(crate) fn goto_local(core: &RouterState, path: &str) {
    let base = core.local_provider.clone();
    if path == "/" || path.is_empty() {
        core.set_active_provider(base, "/".to_string());
    } else {
        let segs = panel_core::parse_path_to_segments(path);
        let levels = panel_core::nav::build_levels(&segs, base.clone());
        *core.path.borrow_mut() = panel_core::nav::NavPath::from_levels(levels, base);
    }
}

async fn run() -> io::Result<()> {
    let config = client_config::AppConfig::new("ice-commander");
    secret_store::harden_file_permissions(&config.config_path());

    let make_core = || {
        let local: Rc<dyn FileSystemRpc> =
            Rc::new(virtualfs::local_rpc::LocalFileSystemRpc::new(config.clone()));
        Rc::new(RouterState::new(local.clone(), local, "/".to_string()))
    };
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".to_string());
    let left = make_core();
    let right = make_core();
    set_start_path(&left, &cwd);
    set_start_path(&right, &cwd);
    let _ = left.list_active().await;
    let _ = right.list_active().await;

    let mut app = App {
        panes: [Pane::new(left), Pane::new(right)],
        config: config.clone(),
        terms: [None, None],
        active: 0,
        focus: Focus::Panel,
        term_expanded: false,
        list_rects: [Rect::default(); 2],
        term_rects: [Rect::default(); 2],
        footer_rects: Vec::new(),
        overlay: overlay::Overlay::None,
        should_quit: false,
    };

    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

enum Wake {
    Key(KeyEvent),
    Click(u16, u16),
    Term(usize, Option<Vec<u8>>),
    Redraw,
    Quit,
}

async fn event_loop(terminal: &mut Tui, app: &mut App) -> io::Result<()> {
    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| ui(f, app))?;
        if app.should_quit {
            break;
        }

        let wake = {
            let [left, right] = &mut app.terms;
            tokio::select! {
                maybe = events.next() => match maybe {
                    Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => Wake::Key(k),
                    Some(Ok(Event::Mouse(m)))
                        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) =>
                    {
                        Wake::Click(m.column, m.row)
                    }
                    Some(Ok(_)) => Wake::Redraw,
                    Some(Err(_)) | None => Wake::Quit,
                },
                out = term_recv(left) => Wake::Term(0, out),
                out = term_recv(right) => Wake::Term(1, out),
            }
        };
        match wake {
            Wake::Key(k) => app.on_key(k).await,
            Wake::Click(col, row) => app.on_click(col, row).await,
            Wake::Term(i, Some(bytes)) => {
                if let Some(t) = &mut app.terms[i] {
                    t.parser.process(&bytes);
                    if bytes.windows(4).any(|w| w == b"\x1b[6n") {
                        let (row, col) = t.parser.screen().cursor_position();
                        let report = format!("\x1b[{};{}R", row + 1, col + 1);
                        let _ = t.input_tx.try_send(report.into_bytes());
                    }
                }
            }
            Wake::Term(i, None) => {
                app.terms[i] = None;
                if app.active == i {
                    app.focus = Focus::Panel;
                }
            }
            Wake::Redraw => {}
            Wake::Quit => break,
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run()).await
}
