use std::error::Error;
use std::io::{self, Write};
use std::time::Duration;

use clap::Parser;
use clock_tui::app::App;
use clock_tui::app::Mode;
use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

struct TerminalSession;

impl TerminalSession {
    fn enter() -> Result<Self, Box<dyn Error>> {
        enable_raw_mode()?;
        if let Err(error) = io::stdout().execute(EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(Box::new(error));
        }
        if let Err(error) = io::stdout().execute(EnableMouseCapture) {
            let _ = io::stdout().execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(Box::new(error));
        }
        Ok(Self)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = io::stdout().execute(Show);
        let _ = io::stdout().execute(DisableMouseCapture);
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments
    // Must be done first so `--help` isn't printed to the alternate screen.
    let mut app = App::parse();

    // Setup terminal. The guard restores raw mode / alternate screen on early errors.
    let terminal_session = TerminalSession::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Load config and initialize app
    app.init_app();

    loop {
        if app.is_ended() {
            break;
        }
        app.tick();
        terminal.draw(|f| app.ui(f))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => break,
                    _ if app.has_widget_popup_open() => app.on_widget_popup_key(key.code),
                    _ if app.open_widget_popup_action(key.code) => {}
                    KeyCode::Char(' ') => app.on_key(KeyCode::Char(' ')),
                    // Display flags given on the command line (title,
                    // seconds, millis, paused, ...) live on `app` and survive
                    // these switches; mode-specific values come from config.
                    KeyCode::Char('c') => app.set_mode(Mode::Clock { timezone: None }),
                    KeyCode::Char('w') => app.set_mode(Mode::Stopwatch),
                    KeyCode::Char('t') => app.set_mode(Mode::Timer {
                        durations: vec![],
                        repeat: false,
                        auto_quit: false,
                        execute: vec![],
                    }),
                    KeyCode::Char('T')
                    | KeyCode::Char('g')
                    | KeyCode::Char('z')
                    | KeyCode::Home
                    | KeyCode::End => app.on_key(key.code),
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.on_mouse_scroll(mouse.column, mouse.row, 1),
                    MouseEventKind::ScrollUp => app.on_mouse_scroll(mouse.column, mouse.row, -1),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // Restore terminal before printing exit messages.
    terminal.show_cursor()?;
    drop(terminal);
    drop(terminal_session);

    // Perform logic such as printing the stopwatch time.
    // Must be done after leaving alternate screen.
    app.on_exit();
    io::stdout().flush()?;

    Ok(())
}
