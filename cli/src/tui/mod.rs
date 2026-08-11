//! `alexandria tui` — an interactive terminal UI over the same impl functions the
//! subcommands call.
//!
//! The CLI is good at scripted, one-shot operations. It is poor at the other
//! half of the work: looking at what is in the store, picking something, and
//! acting on it. Doing that with subcommands means `list`, copy a URN, then
//! `revoke <urn> --reason …` — three steps and a chance to paste the wrong
//! identifier. The TUI collapses that into select-and-act.
//!
//! It is a front end, not a second implementation: every mutation routes
//! through [`crate::vault`] and `app_lib::commands::*`, exactly as the
//! subcommands do.

mod app;
mod clipboard;
mod ui;

use std::io;
use std::panic;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::context::ProjectContext;
use crate::output;
use app::App;

/// How long to block on input before redrawing. Long enough to be idle-cheap,
/// short enough that a resize feels immediate.
const TICK: Duration = Duration::from_millis(200);

pub fn run(ctx: &ProjectContext, password_file: Option<&Path>) -> Result<()> {
    if output::is_json() {
        anyhow::bail!("`alexandria tui` is interactive and has no JSON form — drop --json");
    }
    // Fail before entering the alternate screen. Otherwise the user gets an
    // unlock prompt that no password can satisfy, and the reason why is
    // hidden behind the full-screen UI.
    if !ctx.has_vault() {
        anyhow::bail!(
            "No vault found at {}.\n\
             Launch the app and create a wallet first.",
            ctx.vault_dir().display()
        );
    }

    let mut terminal = setup()?;
    // Run the app with the terminal already restored on the way out,
    // whichever way we leave: error, panic, or a clean quit.
    let result = App::new(ctx.clone(), password_file.map(Path::to_path_buf))
        .and_then(|mut app| event_loop(&mut terminal, &mut app));
    restore()?;
    result
}

/// Enter raw mode and the alternate screen, installing a panic hook that
/// restores the terminal first.
///
/// Without the hook a panic inside the draw code leaves the user with no echo
/// and no cursor in a screen they cannot scroll — the shell looks broken and
/// the backtrace is invisible.
fn setup() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore();
        original_hook(info);
    }));

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    // Bracketed paste matters here rather than being a nicety: pasted JSON
    // contains newlines, and without it the terminal delivers those as Enter
    // keypresses — the form would submit partway through the paste.
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)
        .context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("create terminal")
}

fn restore() -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    Ok(())
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if !event::poll(TICK)? {
            continue;
        }
        match event::read()? {
            // `Press` only: crossterm on Windows reports key release as a
            // separate event, which would otherwise action every key twice.
            Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                app.on_key(key);
                if app.should_quit {
                    return Ok(());
                }
            }
            // One event for the whole paste, however many lines it spans.
            Event::Paste(text) => app.on_paste(&text),
            _ => {}
        }
    }
}
