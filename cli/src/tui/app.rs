//! TUI state and the actions it can take.
//!
//! Every mutation goes through the same `app_lib::commands::*` impl functions
//! the subcommands use — this module decides *what* to call and *when*, never
//! *how* a credential is signed or stored.

use std::path::PathBuf;

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rusqlite::Connection;

use app_lib::commands::role_assessment::{Organization, RoleAssessment};
use app_lib::domain::vc::VerifiableCredential;

use crate::context::ProjectContext;
use crate::tui::clipboard;
use crate::tui::logs;
use crate::vault::{self, Signer};

/// Top-level sections, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Credentials,
    Roles,
    Organizations,
    Database,
    Verify,
    Doctor,
    Logs,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Credentials,
        Tab::Roles,
        Tab::Organizations,
        Tab::Database,
        Tab::Verify,
        Tab::Doctor,
        Tab::Logs,
    ];

    /// Short on purpose: the tab bar has to fit an 80-column terminal with
    /// six entries and their counts.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Credentials => "Credentials",
            Tab::Roles => "Roles",
            Tab::Organizations => "Orgs",
            Tab::Database => "Database",
            Tab::Verify => "Verify",
            Tab::Doctor => "Doctor",
            Tab::Logs => "Logs",
        }
    }

    /// Whether the tab bar shows a row count for this tab. Verify is a menu
    /// and Doctor is a report, so a count would be noise.
    pub fn shows_count(self) -> bool {
        !matches!(self, Tab::Verify | Tab::Doctor | Tab::Logs)
    }

    pub(super) fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn from_index(i: usize) -> Tab {
        Tab::ALL[i % Tab::ALL.len()]
    }
}

/// The two file-driven verifications, listed on the Verify tab.
pub const VERIFY_ENTRIES: [(&str, &str); 2] = [
    (
        "Credential or bundle (offline)",
        "Check a credential, a list of them, or a §20.4 bundle — no infrastructure needed",
    ),
    (
        "Presentation",
        "Check a presentation envelope against an expected audience",
    ),
];

/// Extra guidance shown under the selected verification.
pub const VERIFY_HELP: [&str; 2] = [
    "Paste a credential straight in, or give a path. A bare credential \
     verifies on its own — the issuer's did:key carries the public key — but \
     revocation stays unknown without a bundle. Verify as of: check validity \
     at a past moment; defaults to now.",
    "The audience is required: a presentation built for one verifier is \
     rejected at another rather than silently accepted.",
];

/// What the app is showing. The vault is unlocked once, up front, because
/// every tab except a locked-out Database view needs the database open.
pub enum Screen {
    /// Collecting the vault password.
    Unlock {
        password: String,
        error: Option<String>,
    },
    /// Unlocked and browsing.
    Browse,
}

/// A pending action awaiting confirmation or input.
#[derive(Clone)]
pub enum Pending {
    /// The reason is collected by the form, then carried through the
    /// confirmation step so the user sees it before committing.
    Revoke {
        id: String,
        reason: String,
    },
    Suspend {
        id: String,
    },
    ExportBundle,
    ImportFile,
    CreateOrg,
    IssueRoleCredential {
        assessment_id: String,
    },
    IssueCredential,
    CreateAssessment,
    VerifyBundle,
    VerifyPresentation,
    SetFilter,
    DbSeed,
}

/// A modal over the browse screen.
pub enum Modal {
    None,
    /// Yes/no confirmation for a destructive or irreversible action.
    Confirm {
        title: String,
        body: String,
        action: Pending,
    },
    /// One or more text fields. `Tab` moves between them, `Enter` submits.
    Form {
        title: String,
        fields: Vec<Field>,
        active: usize,
        action: Pending,
    },
    /// A read-only JSON view of the selected record.
    Detail {
        title: String,
        body: String,
        scroll: u16,
    },
    /// The full key reference for the active tab.
    Help,
    /// A page of rows from one database table.
    Rows(TableRows),
    /// One row, every column, values not clipped to fit a grid cell.
    ///
    /// Carries the grid it was opened from so going back restores the exact
    /// cursor and column scroll, rather than re-reading the table and dumping
    /// the reader at the top again.
    Row {
        rows: TableRows,
        pairs: Vec<(String, String)>,
        scroll: u16,
    },
}

/// A window onto a table's contents.
///
/// Rows are fetched with a LIMIT rather than streamed: the point is to look at
/// what is in a table, and holding a million decrypted rows in memory to show
/// twenty of them would be a poor trade. `total` is counted separately so the
/// view can say how much it is not showing.
pub struct TableRows {
    pub table: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total: i64,
    /// The highlighted row. The visible window is derived from it at render
    /// time, so there is no second scroll position to keep in sync.
    pub selected: usize,
    /// First visible column — wide tables scroll sideways.
    pub col_offset: usize,
}

/// How many rows to pull in one look.
const ROW_PAGE: usize = 500;

/// Read a JSON argument that may be either a path or the document itself.
///
/// Pasting a credential straight in is the common case when someone has been
/// sent one; writing it to a file first is friction with no purpose. A value
/// starting with `{` or `[` cannot be a sensible path, so the two are
/// distinguishable without a flag.
pub(super) fn read_json_input(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Ok(trimmed.to_string());
    }
    std::fs::read_to_string(trimmed).with_context(|| format!("read {trimmed}"))
}

/// Flatten and clip one cell for display.
///
/// Newlines and tabs are replaced rather than stripped: a value containing
/// them would otherwise break the row grid apart, and showing a marker is more
/// honest than silently joining the lines.
fn clip(text: &str, limit: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                '⏎'
            } else {
                c
            }
        })
        .collect();
    if flat.chars().count() > limit {
        let kept: String = flat.chars().take(limit).collect();
        format!("{kept}…")
    } else {
        flat
    }
}

/// Longest cell value kept in memory for the grid. Anything longer is
/// truncated on load: a single column holding a base64 credential would
/// otherwise dominate.
const MAX_CELL: usize = 200;

/// Longest value shown when a single row is expanded. Far larger than the grid
/// cap — the point of opening a row is to read what the grid had to cut — but
/// still bounded, so one pathological column cannot lock up the renderer.
const MAX_DETAIL_CELL: usize = 8192;

#[derive(Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    /// Optional fields submit fine when empty; required ones block.
    pub required: bool,
}

impl Field {
    #[cfg(test)]
    pub fn for_test(label: &str, required: bool) -> Self {
        Field::new(label, required)
    }

    fn new(label: &str, required: bool) -> Self {
        Field {
            label: label.to_string(),
            value: String::new(),
            required,
        }
    }
}

/// The outcome of the last verification, kept so the Verify tab can show it
/// after the modal closes.
pub struct VerifyOutcome {
    pub title: String,
    pub lines: Vec<(String, String)>,
    pub ok: bool,
}

pub(crate) use crate::commands::doctor::Section as DoctorSection;

/// The modifier that bypasses mouse reporting so the terminal selects text
/// itself. Terminals differ, and the app cannot ask which one it is in, so
/// this is the convention for the platform rather than a certainty.
pub fn select_modifier() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌥ option"
    } else {
        "shift"
    }
}

/// Where things were drawn last frame, so a click can be resolved to what the
/// user actually pointed at.
///
/// Recorded during render rather than recomputed here: the layout is decided
/// by the draw code, and a second copy of that arithmetic would drift the
/// moment either side changed.
#[derive(Default)]
pub struct Hitboxes {
    /// Horizontal span of each tab label in the bar, with its tab.
    pub tabs: Vec<(u16, u16, Tab)>,
    /// Row of the tab bar.
    pub tab_row: u16,
    /// The list pane's inner area (inside its border).
    pub list: Option<ratatui::layout::Rect>,
    /// Index of the first list row on screen, for turning a y into a row.
    pub list_offset: usize,
}

/// A transient message shown in the status bar.
pub struct Toast {
    pub text: String,
    pub is_error: bool,
}

pub struct App {
    pub ctx: ProjectContext,
    pub password_file: Option<PathBuf>,
    pub screen: Screen,
    pub tab: Tab,
    pub modal: Modal,
    pub toast: Option<Toast>,
    pub should_quit: bool,

    /// Present once unlocked. Holds the open connection and the issuer key,
    /// so signing actions do not re-prompt.
    signer: Option<Signer>,

    pub credentials: Vec<VerifiableCredential>,
    pub assessments: Vec<RoleAssessment>,
    pub organizations: Vec<Organization>,
    pub db_tables: Vec<(String, i64)>,

    /// Substring filter applied to the credentials list. Matches id, issuer,
    /// subject, and type, so one box covers every way a user might recall a
    /// credential.
    pub filter: Option<String>,
    /// Applied and latest schema versions, for the Database detail pane.
    pub migration: Option<(i64, i64)>,
    /// Last verification result, shown on the Verify tab.
    pub verify_result: Option<VerifyOutcome>,
    /// Doctor sections, populated on first visit to the tab.
    pub doctor: Vec<DoctorSection>,
    /// Lowest level shown on the Logs tab.
    pub log_level: log::LevelFilter,
    /// Whether the log view sticks to the newest line.
    pub log_follow: bool,
    /// Scroll position when not following.
    pub log_offset: usize,
    /// Geometry from the last frame, for hit-testing clicks.
    pub hits: Hitboxes,
    /// List scroll state, kept across frames so its offset can be read back
    /// when resolving a click to a row.
    pub list_state: ratatui::widgets::ListState,
    /// Whether the terminal's mouse reporting is on.
    ///
    /// Off by default, deliberately. Enabling it routes button presses to this
    /// program, which stops the terminal doing its own drag-to-select — the
    /// thing that makes `tmux mouse on` infamous. Losing text selection by
    /// default to gain row-clicking is a bad trade for a tool people reach for
    /// when something is wrong and they want to copy an error out of it.
    pub mouse_enabled: bool,

    pub selected: [usize; Tab::ALL.len()],
}

impl App {
    pub fn new(ctx: ProjectContext, password_file: Option<PathBuf>) -> Result<Self> {
        let mut app = App {
            ctx,
            password_file,
            screen: Screen::Unlock {
                password: String::new(),
                error: None,
            },
            tab: Tab::Credentials,
            modal: Modal::None,
            toast: None,
            should_quit: false,
            signer: None,
            credentials: Vec::new(),
            assessments: Vec::new(),
            organizations: Vec::new(),
            db_tables: Vec::new(),
            filter: None,
            migration: None,
            verify_result: None,
            doctor: Vec::new(),
            log_level: log::LevelFilter::Debug,
            log_follow: true,
            log_offset: 0,
            hits: Hitboxes::default(),
            list_state: ratatui::widgets::ListState::default(),
            mouse_enabled: false,
            selected: [0; Tab::ALL.len()],
        };

        // A password file is the non-interactive path — honour it immediately
        // rather than showing an unlock prompt the caller cannot answer.
        if app.password_file.is_some() {
            let password = vault::get_password(app.password_file.as_deref())?;
            app.try_unlock(&password);
        }
        Ok(app)
    }

    fn conn(&self) -> Option<&Connection> {
        self.signer.as_ref().map(|s| &s.conn)
    }

    fn toast_ok(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            is_error: false,
        });
    }

    fn toast_err(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            is_error: true,
        });
    }

    // ---- Unlock ---------------------------------------------------------

    fn try_unlock(&mut self, password: &str) {
        match vault::unlock_with_password(&self.ctx, password) {
            Ok(signer) => {
                log::info!("vault unlocked for profile {}", self.ctx.profile.label());
                self.signer = Some(signer);
                self.screen = Screen::Browse;
                self.refresh();
            }
            Err(e) => {
                // Logged as well as shown: the on-screen message is one line,
                // and the chain behind it is usually where the answer is.
                log::warn!("vault unlock failed: {e:#}");
                self.screen = Screen::Unlock {
                    password: String::new(),
                    error: Some(format!("{e:#}")),
                };
            }
        }
    }

    // ---- Data loading ---------------------------------------------------

    /// Reload every list from the database. Called after unlock and after any
    /// mutation, so the view can never show state the store has moved past.
    pub fn refresh(&mut self) {
        // Every query runs first, while `conn` is borrowed; the results are
        // owned, so the borrow has ended by the time we assign or toast.
        let Some(conn) = self.conn() else { return };
        let creds = app_lib::commands::credentials::list_credentials_impl(conn, None, None);
        let assessments =
            app_lib::commands::role_assessment::list_role_assessments_impl(conn, None);
        let orgs = app_lib::commands::role_assessment::list_organizations_impl(conn, None);

        match creds {
            Ok(list) => self.credentials = list,
            Err(e) => self.toast_err(format!("list credentials: {e}")),
        }
        match assessments {
            Ok(list) => self.assessments = list,
            Err(e) => self.toast_err(format!("list assessments: {e}")),
        }
        match orgs {
            Ok(list) => self.organizations = list,
            Err(e) => self.toast_err(format!("list organizations: {e}")),
        }
        self.load_db_tables();
        self.load_migration_version();
        self.clamp_selection();
        log::debug!(
            "refreshed: {} credentials, {} assessments, {} organizations, {} tables",
            self.credentials.len(),
            self.assessments.len(),
            self.organizations.len(),
            self.db_tables.len()
        );
    }

    fn load_migration_version(&mut self) {
        let Some(conn) = self.conn() else { return };
        let latest = crate::commands::db::latest_version();
        // A database with no _migrations table yet reports v0 rather than
        // erroring — that is the honest answer before the first migrate.
        let current = crate::commands::db::current_version(conn);
        self.migration = Some((current, latest));
    }

    fn load_db_tables(&mut self) {
        let Some(conn) = self.conn() else { return };
        let mut tables = Vec::new();

        let names: Result<Vec<String>, rusqlite::Error> = (|| {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect()
        })();

        let outcome = names.map(|names| {
            for name in names {
                // Table names come from sqlite_master, not user input, so
                // interpolating them here cannot inject. A table we cannot
                // count (a virtual table, say) reports -1 rather than
                // aborting the whole listing.
                let count = conn
                    .query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap_or(-1);
                tables.push((name, count));
            }
            tables
        });

        match outcome {
            Ok(tables) => self.db_tables = tables,
            Err(e) => self.toast_err(format!("read schema: {e}")),
        }
    }

    // ---- Selection ------------------------------------------------------

    fn list_len(&self, tab: Tab) -> usize {
        match tab {
            Tab::Credentials => self.visible_credentials().len(),
            Tab::Roles => self.assessments.len(),
            Tab::Organizations => self.organizations.len(),
            Tab::Database => self.db_tables.len(),
            Tab::Verify => VERIFY_ENTRIES.len(),
            Tab::Doctor => self.doctor.len(),
            // The log view scrolls rather than selecting, so it has no rows
            // for the shared cursor to index.
            Tab::Logs => 0,
        }
    }

    /// Credentials after the active filter. The filter is a plain substring
    /// match, case-insensitive, across the fields a user is likely to
    /// remember — one box rather than a subject/skill/type choice up front.
    pub fn visible_credentials(&self) -> Vec<&VerifiableCredential> {
        let Some(needle) = self.filter.as_ref().map(|f| f.to_lowercase()) else {
            return self.credentials.iter().collect();
        };
        self.credentials
            .iter()
            .filter(|vc| {
                let haystacks = [
                    vc.id.clone().unwrap_or_default(),
                    vc.issuer.as_str().to_string(),
                    vc.credential_subject.id.as_str().to_string(),
                    vc.type_.join(" "),
                ];
                haystacks.iter().any(|h| h.to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn selected_index(&self) -> usize {
        self.selected[self.tab.index()]
    }

    fn clamp_selection(&mut self) {
        for tab in Tab::ALL {
            let len = self.list_len(tab);
            let idx = &mut self.selected[tab.index()];
            *idx = if len == 0 { 0 } else { (*idx).min(len - 1) };
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.list_len(self.tab);
        if len == 0 {
            return;
        }
        let i = self.selected_index() as isize + delta;
        let wrapped = i.rem_euclid(len as isize) as usize;
        self.selected[self.tab.index()] = wrapped;
    }

    pub fn selected_credential(&self) -> Option<&VerifiableCredential> {
        self.visible_credentials()
            .get(self.selected_index())
            .copied()
    }

    pub fn selected_assessment(&self) -> Option<&RoleAssessment> {
        self.assessments.get(self.selected_index())
    }

    pub fn selected_organization(&self) -> Option<&Organization> {
        self.organizations.get(self.selected_index())
    }

    /// The one-line footer. Deliberately short: the full key list lives
    /// behind `?`, because a bar long enough to name every action is wider
    /// than an 80-column terminal and silently loses its tail — including
    /// the key that quits.
    pub fn hints(&self) -> &'static str {
        match self.modal {
            // The log stream scrolls rather than selecting, and has its own
            // controls; advertising "⏎ detail" there points at nothing.
            Modal::None if self.tab == Tab::Logs => {
                "↑↓ scroll · f follow · l level · c clear · y copy · ? keys · q quit"
            }
            Modal::None => "↑↓ move · ⏎ detail · y copy · ? keys · q quit",
            Modal::Confirm { .. } => "y confirm · n/esc cancel",
            Modal::Form { .. } => "⇥ field · ⏎ submit · esc cancel",
            Modal::Detail { .. } => "↑↓ scroll · y copy · esc close",
            Modal::Rows(_) => "↑↓ rows · ←→ columns · ⏎ full row · esc close",
            Modal::Row { .. } => "↑↓ scroll · y copy · esc back",
            Modal::Help => "esc close",
        }
    }

    /// The full key reference for the active tab, shown by `?`.
    pub fn help_lines(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![
            ("↑ ↓ / j k", "move the selection"),
            ("⇥ / ⇧⇥", "next / previous tab"),
            ("1 … 7", "jump straight to a tab"),
            ("⏎", "open the full record"),
            ("g", "refresh from the database"),
            ("y", "copy what is on screen as JSON"),
            ("M", "mouse on/off (off keeps drag-to-select working)"),
            ("?", "this help"),
            ("q / esc", "quit"),
            ("ctrl-c", "quit from anywhere"),
        ];
        keys.extend(match self.tab {
            Tab::Credentials => vec![
                ("n", "issue a credential from a request file"),
                ("r", "revoke (permanent, asks to confirm)"),
                ("s", "suspend (reversible)"),
                ("i", "reinstate a suspended credential"),
                ("e", "export a survivability bundle"),
                ("m", "import credentials from a file"),
                ("/", "filter the list (esc clears)"),
            ],
            Tab::Roles => vec![
                ("n", "new role assessment from a request file"),
                ("p", "publish"),
                ("a", "archive"),
                ("d", "back to draft"),
                ("c", "issue a role credential"),
            ],
            Tab::Organizations => vec![("n", "new organization")],
            Tab::Database => vec![
                (
                    "⏎",
                    "browse the rows in this table, then ⏎ again for one row",
                ),
                ("m", "run pending migrations"),
                ("s", "seed demo data if the database is empty"),
            ],
            Tab::Verify => vec![("⏎", "run the selected verification")],
            Tab::Doctor => vec![("g", "re-run the checks")],
            Tab::Logs => vec![
                ("↑↓ / PgUp PgDn", "scroll"),
                ("f", "follow the newest line (on/off)"),
                ("l", "cycle level: error, warn, info, debug"),
                ("c", "clear"),
                ("y", "copy what is shown"),
            ],
        });
        keys
    }

    // ---- Key handling ---------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        // Ctrl-C always quits, whatever is on screen.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match &mut self.screen {
            Screen::Unlock { .. } => self.on_key_unlock(key),
            Screen::Browse => match self.modal {
                Modal::None => self.on_key_browse(key),
                _ => self.on_key_modal(key),
            },
        }
    }

    /// Insert pasted text into whatever is currently accepting input.
    ///
    /// Arrives as one event even when it spans many lines, so a pasted
    /// credential does not submit the form at its first newline.
    pub fn on_paste(&mut self, text: &str) {
        match &mut self.screen {
            Screen::Unlock { password, .. } => {
                // A pasted password should not carry a trailing newline from
                // the clipboard into the key derivation.
                password.push_str(text.trim_end_matches(['\n', '\r']));
            }
            Screen::Browse => {
                if let Modal::Form { fields, active, .. } = &mut self.modal {
                    fields[*active].value.push_str(text);
                }
            }
        }
    }

    /// Toggle terminal mouse reporting.
    ///
    /// Returns whether it is now on, so the caller can tell the terminal —
    /// the escape sequences are the event loop's business, not this module's.
    pub fn toggle_mouse(&mut self) -> bool {
        self.mouse_enabled = !self.mouse_enabled;
        if self.mouse_enabled {
            self.toast_ok(format!(
                "Mouse on — hold {} to select text",
                select_modifier()
            ));
        } else {
            self.toast_ok("Mouse off — drag to select text as usual");
        }
        log::info!("mouse reporting {}", self.mouse_enabled);
        self.mouse_enabled
    }

    /// Resolve a mouse event against the geometry of the last frame.
    pub fn on_mouse(&mut self, event: MouseEvent) {
        if !self.mouse_enabled {
            return;
        }
        // The unlock screen has nothing to click, and a stray click there
        // should not dismiss the prompt.
        if matches!(self.screen, Screen::Unlock { .. }) {
            return;
        }

        match event.kind {
            MouseEventKind::ScrollDown => self.scroll_by(1),
            MouseEventKind::ScrollUp => self.scroll_by(-1),
            MouseEventKind::Down(MouseButton::Left) => self.click(event.column, event.row),
            _ => {}
        }
    }

    /// Wheel scrolling, routed to whatever is in front.
    fn scroll_by(&mut self, delta: isize) {
        match &mut self.modal {
            Modal::Detail { scroll, .. } | Modal::Row { scroll, .. } => {
                *scroll = if delta > 0 {
                    scroll.saturating_add(1)
                } else {
                    scroll.saturating_sub(1)
                };
            }
            Modal::Rows(view) => {
                let last = view.rows.len().saturating_sub(1);
                view.selected = if delta > 0 {
                    (view.selected + 1).min(last)
                } else {
                    view.selected.saturating_sub(1)
                };
            }
            Modal::Help | Modal::Confirm { .. } | Modal::Form { .. } => {}
            Modal::None => {
                if self.tab == Tab::Logs {
                    // Scrolling means the reader has taken over, same as with
                    // the keyboard.
                    self.log_follow = false;
                    let total = logs::entries(self.log_level).len();
                    self.log_offset = if delta > 0 {
                        (self.log_offset + 1).min(total.saturating_sub(1))
                    } else {
                        self.log_offset.saturating_sub(1)
                    };
                } else {
                    self.move_selection(delta);
                }
            }
        }
    }

    fn click(&mut self, column: u16, row: u16) {
        // A modal owns the screen; clicking through it to the list behind
        // would act on something the user cannot see.
        if !matches!(self.modal, Modal::None) {
            return;
        }

        if row == self.hits.tab_row {
            if let Some((_, _, tab)) = self
                .hits
                .tabs
                .iter()
                .find(|(start, end, _)| column >= *start && column < *end)
            {
                self.tab = *tab;
                self.on_tab_entered();
            }
            return;
        }

        let Some(area) = self.hits.list else { return };
        let inside = column >= area.x
            && column < area.x + area.width
            && row >= area.y
            && row < area.y + area.height;
        if !inside {
            return;
        }

        let clicked = self.hits.list_offset + (row - area.y) as usize;
        if clicked >= self.list_len(self.tab) {
            return;
        }

        // Clicking the highlighted row opens it — one click to aim, a second
        // to act, so a misplaced click cannot trigger anything.
        if clicked == self.selected_index() {
            if self.tab == Tab::Verify {
                self.open_verify_form();
            } else {
                self.open_detail();
            }
        } else {
            self.selected[self.tab.index()] = clicked;
        }
    }

    fn on_key_unlock(&mut self, key: KeyEvent) {
        let Screen::Unlock { password, .. } = &mut self.screen else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Enter => {
                let entered = password.clone();
                self.try_unlock(&entered);
            }
            KeyCode::Backspace => {
                password.pop();
            }
            KeyCode::Char(c) => password.push(c),
            _ => {}
        }
    }

    fn on_key_browse(&mut self, key: KeyEvent) {
        // Any keypress dismisses a stale toast.
        self.toast = None;

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            // Esc clears a filter first — quitting out from under a filtered
            // view is almost never what the key was meant to do.
            KeyCode::Esc => {
                if self.tab == Tab::Credentials && self.filter.is_some() {
                    self.filter = None;
                    self.clamp_selection();
                    self.toast_ok("Filter cleared");
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Tab | KeyCode::Right => {
                self.tab = Tab::from_index(self.tab.index() + 1);
                self.on_tab_entered();
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.tab = Tab::from_index(self.tab.index() + Tab::ALL.len() - 1);
                self.on_tab_entered();
            }
            // The log stream scrolls instead of selecting, so it takes the
            // arrows itself; without the guard these are swallowed here and
            // the tab appears frozen.
            KeyCode::Down | KeyCode::Char('j') if self.tab != Tab::Logs => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') if self.tab != Tab::Logs => self.move_selection(-1),
            KeyCode::Char('g') => {
                if self.tab == Tab::Doctor {
                    self.run_doctor();
                    self.toast_ok("Checks re-run");
                } else {
                    self.refresh();
                    self.toast_ok("Refreshed");
                }
            }
            // Jump straight to a tab. Digits are bound nowhere else, and the
            // numbers are shown in the tab bar so they are discoverable.
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let index = c as usize - '1' as usize;
                if index < Tab::ALL.len() {
                    self.tab = Tab::ALL[index];
                    self.on_tab_entered();
                }
            }
            // Capital M: lowercase `m` already means import on Credentials
            // and migrate on Database, and a key that means different things
            // depending on where you are is worse than no key at all.
            KeyCode::Char('M') => {
                self.toggle_mouse();
            }
            KeyCode::Char('?') => self.modal = Modal::Help,
            KeyCode::Char('y') => self.copy_current(),
            KeyCode::Enter => {
                if self.tab == Tab::Verify {
                    self.open_verify_form();
                } else {
                    self.open_detail();
                }
            }
            _ => match self.tab {
                Tab::Credentials => self.on_key_credentials(key),
                Tab::Roles => self.on_key_roles(key),
                Tab::Organizations => self.on_key_orgs(key),
                Tab::Database => self.on_key_database(key),
                Tab::Logs => self.on_key_logs(key),
                Tab::Verify | Tab::Doctor => {}
            },
        }
    }

    fn on_key_credentials(&mut self, key: KeyEvent) {
        let selected_id = self.selected_credential().and_then(|vc| vc.id.clone());

        match key.code {
            KeyCode::Char('r') => {
                let Some(id) = selected_id else {
                    return self.toast_err("That credential has no envelope id");
                };
                self.modal = Modal::Form {
                    title: format!("Revoke {id}"),
                    fields: vec![Field::new("Reason", true)],
                    active: 0,
                    action: Pending::Revoke {
                        id,
                        reason: String::new(),
                    },
                };
            }
            KeyCode::Char('s') => {
                let Some(id) = selected_id else {
                    return self.toast_err("That credential has no envelope id");
                };
                self.modal = Modal::Form {
                    title: format!("Suspend {id}"),
                    fields: vec![
                        Field::new("Until (ISO 8601, optional)", false),
                        Field::new("Reason (optional)", false),
                    ],
                    active: 0,
                    action: Pending::Suspend { id },
                };
            }
            KeyCode::Char('i') => {
                let Some(id) = selected_id else {
                    return self.toast_err("That credential has no envelope id");
                };
                self.reinstate(&id);
            }
            KeyCode::Char('e') => {
                self.modal = Modal::Form {
                    title: "Export survivability bundle".into(),
                    fields: vec![Field::new("Output path", true)],
                    active: 0,
                    action: Pending::ExportBundle,
                };
            }
            KeyCode::Char('m') => {
                self.modal = Modal::Form {
                    title: "Import credentials".into(),
                    fields: vec![Field::new("File (path or pasted JSON)", true)],
                    active: 0,
                    action: Pending::ImportFile,
                };
            }
            KeyCode::Char('n') => {
                self.modal = Modal::Form {
                    title: "Issue credential".into(),
                    fields: vec![Field::new("Request (path or pasted JSON)", true)],
                    active: 0,
                    action: Pending::IssueCredential,
                };
            }
            KeyCode::Char('/') => {
                let mut field = Field::new("Match id, issuer, subject, or type", false);
                field.value = self.filter.clone().unwrap_or_default();
                self.modal = Modal::Form {
                    title: "Filter credentials".into(),
                    fields: vec![field],
                    active: 0,
                    action: Pending::SetFilter,
                };
            }
            _ => {}
        }
    }

    /// Doctor's checks shell out, so they run on first arrival rather than on
    /// every refresh of every other tab.
    fn on_tab_entered(&mut self) {
        if self.tab == Tab::Doctor && self.doctor.is_empty() {
            self.run_doctor();
        }
    }

    fn on_key_logs(&mut self, key: KeyEvent) {
        let total = logs::entries(self.log_level).len();
        match key.code {
            // Scrolling means the reader has taken over; following would yank
            // the view back to the bottom on the next record.
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_follow = false;
                self.log_offset = self.log_offset.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_follow = false;
                self.log_offset = (self.log_offset + 1).min(total.saturating_sub(1));
            }
            KeyCode::PageUp => {
                self.log_follow = false;
                self.log_offset = self.log_offset.saturating_sub(20);
            }
            KeyCode::PageDown => {
                self.log_follow = false;
                self.log_offset = (self.log_offset + 20).min(total.saturating_sub(1));
            }
            KeyCode::Home => {
                self.log_follow = false;
                self.log_offset = 0;
            }
            KeyCode::End => self.log_follow = true,
            KeyCode::Char('f') => {
                self.log_follow = !self.log_follow;
                let state = if self.log_follow { "on" } else { "off" };
                self.toast_ok(format!("Follow {state}"));
            }
            KeyCode::Char('l') => {
                use log::LevelFilter::*;
                self.log_level = match self.log_level {
                    Error => Warn,
                    Warn => Info,
                    Info => Debug,
                    _ => Error,
                };
                self.log_offset = 0;
                self.toast_ok(format!("Showing {} and above", self.log_level));
            }
            KeyCode::Char('c') => {
                logs::clear();
                self.log_offset = 0;
                self.toast_ok("Log cleared");
            }
            _ => {}
        }
    }

    fn on_key_database(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('m') => self.run_migrate(),
            KeyCode::Char('s') => {
                self.modal = Modal::Confirm {
                    title: "Seed demo data?".into(),
                    body: "Inserts the demo taxonomy, courses, and governance rows \
                           if the database is empty. Existing data is left alone."
                        .into(),
                    action: Pending::DbSeed,
                };
            }
            _ => {}
        }
    }

    fn open_verify_form(&mut self) {
        match self.selected_index() {
            0 => {
                self.modal = Modal::Form {
                    title: "Verify credential bundle".into(),
                    fields: vec![
                        Field::new("Bundle (path or pasted JSON)", true),
                        Field::new("Verify as of (default: now)", false),
                    ],
                    active: 0,
                    action: Pending::VerifyBundle,
                };
            }
            _ => {
                self.modal = Modal::Form {
                    title: "Verify presentation".into(),
                    fields: vec![
                        Field::new("Envelope (path or pasted JSON)", true),
                        Field::new("Expected audience", true),
                    ],
                    active: 0,
                    action: Pending::VerifyPresentation,
                };
            }
        }
    }

    fn on_key_roles(&mut self, key: KeyEvent) {
        let Some(ra) = self.selected_assessment() else {
            return;
        };
        let id = ra.id.clone();

        let status = match key.code {
            KeyCode::Char('p') => Some("published"),
            KeyCode::Char('a') => Some("archived"),
            KeyCode::Char('d') => Some("draft"),
            _ => None,
        };
        if let Some(status) = status {
            return self.set_role_status(&id, status);
        }

        if key.code == KeyCode::Char('n') {
            self.modal = Modal::Form {
                title: "New role assessment".into(),
                fields: vec![Field::new("Request (path or pasted JSON)", true)],
                active: 0,
                action: Pending::CreateAssessment,
            };
            return;
        }

        if key.code == KeyCode::Char('c') {
            self.modal = Modal::Form {
                title: format!("Issue role credential for {id}"),
                fields: vec![
                    Field::new("Subject DID", true),
                    Field::new("Integrity session id", true),
                ],
                active: 0,
                action: Pending::IssueRoleCredential { assessment_id: id },
            };
        }
    }

    fn on_key_orgs(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('n') {
            self.modal = Modal::Form {
                title: "New organization".into(),
                fields: vec![
                    Field::new("Name", true),
                    Field::new("Owner address", true),
                    Field::new("DID (optional)", false),
                ],
                active: 0,
                action: Pending::CreateOrg,
            };
        }
    }

    fn on_key_modal(&mut self, key: KeyEvent) {
        match &mut self.modal {
            Modal::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.modal = Modal::None;
                }
            }
            Modal::Rows(view) => {
                let last = view.rows.len().saturating_sub(1);
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.modal = Modal::None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        view.selected = (view.selected + 1).min(last)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        view.selected = view.selected.saturating_sub(1)
                    }
                    KeyCode::PageDown => view.selected = (view.selected + 20).min(last),
                    KeyCode::PageUp => view.selected = view.selected.saturating_sub(20),
                    KeyCode::Home => view.selected = 0,
                    KeyCode::End => view.selected = last,
                    KeyCode::Right | KeyCode::Char('l') => {
                        // Always leave one column visible.
                        let max = view.columns.len().saturating_sub(1);
                        view.col_offset = (view.col_offset + 1).min(max);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        view.col_offset = view.col_offset.saturating_sub(1)
                    }
                    KeyCode::Enter => self.open_row_detail(),
                    _ => {}
                }
            }
            Modal::Row { scroll, .. } => match key.code {
                KeyCode::Char('y') => self.copy_current(),
                // Esc goes back to the grid, not out of the browser entirely.
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => self.reopen_rows(),
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::Home => *scroll = 0,
                _ => {}
            },
            Modal::Detail { scroll, .. } => match key.code {
                KeyCode::Char('y') => self.copy_current(),
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.modal = Modal::None,
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                _ => {}
            },
            Modal::Confirm { action, .. } => match key.code {
                KeyCode::Char('y') => {
                    let action = action.clone();
                    self.modal = Modal::None;
                    self.perform(action, Vec::new());
                }
                KeyCode::Char('n') | KeyCode::Esc => self.modal = Modal::None,
                _ => {}
            },
            Modal::Form {
                fields,
                active,
                action,
                ..
            } => match key.code {
                KeyCode::Esc => self.modal = Modal::None,
                KeyCode::Tab | KeyCode::Down => *active = (*active + 1) % fields.len(),
                KeyCode::BackTab | KeyCode::Up => {
                    *active = (*active + fields.len() - 1) % fields.len()
                }
                KeyCode::Backspace => {
                    fields[*active].value.pop();
                }
                KeyCode::Char(c) => fields[*active].value.push(c),
                KeyCode::Enter => {
                    if let Some(missing) = fields
                        .iter()
                        .find(|f| f.required && f.value.trim().is_empty())
                    {
                        let label = missing.label.clone();
                        return self.toast_err(format!("`{label}` is required"));
                    }
                    let values: Vec<String> =
                        fields.iter().map(|f| f.value.trim().to_string()).collect();
                    let action = action.clone();
                    self.modal = Modal::None;
                    self.confirm_or_perform(action, values);
                }
                _ => {}
            },
            Modal::None => {}
        }
    }

    /// Revocation is permanent, so it gets a second gate after the reason is
    /// typed. Everything else runs straight away.
    fn confirm_or_perform(&mut self, action: Pending, values: Vec<String>) {
        if let Pending::Revoke { id, .. } = &action {
            let reason = values.first().cloned().unwrap_or_default();
            self.modal = Modal::Confirm {
                title: "Revoke permanently?".into(),
                body: format!(
                    "{id}\n\nRevocation is written to the status list and cannot be undone. \
                     Use suspend if you may want to reinstate later.\n\nReason: {reason}"
                ),
                action: Pending::Revoke {
                    id: id.clone(),
                    reason,
                },
            };
            return;
        }
        self.perform(action, values);
    }

    // ---- Actions --------------------------------------------------------

    fn perform(&mut self, action: Pending, values: Vec<String>) {
        let result = match action {
            Pending::Revoke { id, reason } => self.revoke(&id, &reason),
            Pending::Suspend { id } => {
                let until = values.first().filter(|s| !s.is_empty()).cloned();
                let reason = values.get(1).filter(|s| !s.is_empty()).cloned();
                self.suspend(&id, until.as_deref(), reason.as_deref())
            }
            Pending::ExportBundle => self.export_bundle(&values[0]),
            Pending::ImportFile => self.import_file(&values[0]),
            Pending::CreateOrg => {
                let did = values.get(2).filter(|s| !s.is_empty()).cloned();
                self.create_org(&values[0], &values[1], did.as_deref())
            }
            Pending::IssueRoleCredential { assessment_id } => {
                self.issue_role_credential(&assessment_id, &values[0], &values[1])
            }
            Pending::IssueCredential => self.issue_credential(&values[0]),
            Pending::CreateAssessment => self.create_assessment(&values[0]),
            Pending::VerifyBundle => {
                let at = values.get(1).filter(|s| !s.is_empty()).cloned();
                self.verify_bundle(&values[0], at.as_deref())
            }
            Pending::VerifyPresentation => self.verify_presentation(&values[0], &values[1]),
            Pending::DbSeed => self.db_seed(),
            Pending::SetFilter => {
                // Filtering touches no state the lists are derived from, so it
                // skips the refresh every other action triggers.
                let needle = values.first().cloned().unwrap_or_default();
                self.filter = if needle.is_empty() {
                    None
                } else {
                    Some(needle)
                };
                self.clamp_selection();
                let count = self.visible_credentials().len();
                match &self.filter {
                    Some(f) => self.toast_ok(format!("{count} match \"{f}\" — esc clears")),
                    None => self.toast_ok("Filter cleared"),
                }
                return;
            }
        };

        match result {
            Ok(msg) => {
                log::info!("{msg}");
                self.refresh();
                self.toast_ok(msg);
            }
            Err(e) => {
                log::error!("action failed: {e:#}");
                self.toast_err(format!("{e:#}"))
            }
        }
    }

    fn revoke(&mut self, id: &str, reason: &str) -> Result<String> {
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        app_lib::commands::credentials::revoke_credential_impl(conn, id, reason, &now)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!("Revoked {id}"))
    }

    fn suspend(&mut self, id: &str, until: Option<&str>, reason: Option<&str>) -> Result<String> {
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        app_lib::commands::credentials::suspend_credential_impl(conn, id, until, reason, &now)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!("Suspended {id}"))
    }

    fn reinstate(&mut self, id: &str) {
        let result = (|| -> Result<String> {
            let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
            app_lib::commands::credentials::reinstate_credential_impl(conn, id)
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(format!("Reinstated {id}"))
        })();
        match result {
            Ok(msg) => {
                self.refresh();
                self.toast_ok(msg);
            }
            Err(e) => self.toast_err(format!("{e:#}")),
        }
    }

    fn export_bundle(&mut self, path: &str) -> Result<String> {
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let bundle = app_lib::commands::credentials::export_bundle_impl(conn)
            .map_err(|e| anyhow::anyhow!(e))?;
        std::fs::write(path, &bundle)?;
        Ok(format!("Wrote {} bytes to {path}", bundle.len()))
    }

    fn import_file(&mut self, path: &str) -> Result<String> {
        let payload = read_json_input(path)?;
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let summary = app_lib::commands::import::import_credentials_impl(conn, &payload, &now)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!(
            "Imported {}, already present {}, failed {}",
            summary.imported,
            summary.already_present,
            summary.failed.len()
        ))
    }

    fn create_org(&mut self, name: &str, owner: &str, did: Option<&str>) -> Result<String> {
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let org = app_lib::commands::role_assessment::create_organization_impl(
            conn, name, owner, did, &now,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!("Created organization {}", org.id))
    }

    fn set_role_status(&mut self, id: &str, status: &str) {
        let result = (|| -> Result<String> {
            let now = vault::now_rfc3339();
            let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
            let ra = app_lib::commands::role_assessment::set_role_assessment_status_impl(
                conn, id, status, &now,
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            Ok(format!("{} is now {}", ra.role_title, ra.status))
        })();
        match result {
            Ok(msg) => {
                self.refresh();
                self.toast_ok(msg);
            }
            Err(e) => self.toast_err(format!("{e:#}")),
        }
    }

    fn issue_role_credential(
        &mut self,
        assessment_id: &str,
        subject: &str,
        session: &str,
    ) -> Result<String> {
        let now = vault::now_rfc3339();
        let subject_did = app_lib::crypto::did::Did(subject.to_string());
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("vault locked"))?;

        let vc = app_lib::commands::role_assessment::issue_role_credential_impl(
            &signer.conn,
            &signer.signing_key,
            &signer.issuer_did,
            assessment_id,
            &subject_did,
            session,
            &now,
        )
        .map_err(|e| anyhow::anyhow!(e))?;

        Ok(format!(
            "Issued {}",
            vc.id.as_deref().unwrap_or("(no envelope id)")
        ))
    }

    /// Test-only constructor: an unlocked-looking app with no signer behind
    /// it, so any test that accidentally reaches the backend fails loudly
    /// instead of silently passing.
    #[cfg(test)]
    pub fn for_test(ctx: ProjectContext) -> Self {
        App {
            ctx,
            password_file: None,
            screen: Screen::Browse,
            tab: Tab::Credentials,
            modal: Modal::None,
            toast: None,
            should_quit: false,
            signer: None,
            credentials: Vec::new(),
            assessments: Vec::new(),
            organizations: Vec::new(),
            db_tables: Vec::new(),
            filter: None,
            migration: None,
            verify_result: None,
            doctor: Vec::new(),
            log_level: log::LevelFilter::Debug,
            log_follow: true,
            log_offset: 0,
            hits: Hitboxes::default(),
            list_state: ratatui::widgets::ListState::default(),
            mouse_enabled: false,
            selected: [0; Tab::ALL.len()],
        }
    }

    #[cfg(test)]
    pub fn set_toast_for_test(&mut self, text: &str, is_error: bool) {
        self.toast = Some(Toast {
            text: text.to_string(),
            is_error,
        });
    }

    fn issue_credential(&mut self, request_path: &str) -> Result<String> {
        let raw = read_json_input(request_path)?;
        let req: app_lib::commands::credentials::IssueCredentialRequest =
            serde_json::from_str(&raw)?;
        let now = vault::now_rfc3339();
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("vault locked"))?;

        let vc = app_lib::commands::credentials::issue_credential_impl(
            &signer.conn,
            &signer.signing_key,
            &signer.issuer_did,
            &req,
            &now,
        )
        .map_err(|e| anyhow::anyhow!(e))?;

        Ok(format!(
            "Issued {}",
            vc.id.as_deref().unwrap_or("(no envelope id)")
        ))
    }

    fn create_assessment(&mut self, request_path: &str) -> Result<String> {
        let raw = read_json_input(request_path)?;
        let req: app_lib::commands::role_assessment::CreateRoleAssessmentRequest =
            serde_json::from_str(&raw)?;
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let ra = app_lib::commands::role_assessment::create_role_assessment_impl(conn, &req, &now)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!("Created {}", ra.role_title))
    }

    fn verify_bundle(&mut self, path: &str, at: Option<&str>) -> Result<String> {
        use app_lib::commands::credentials::OfflineSource;

        let json = read_json_input(path)?;
        let now = at.map(str::to_string).unwrap_or_else(vault::now_rfc3339);
        let report = app_lib::commands::credentials::verify_offline_impl(&json, &now)
            .map_err(|e| anyhow::anyhow!(e))?;

        let kind = match report.source {
            OfflineSource::Bundle => "§20.4 bundle",
            OfflineSource::Credential => "single credential",
            OfflineSource::Credentials => "list of credentials",
        };

        let mut lines = vec![
            ("Input".into(), kind.to_string()),
            ("At".into(), now),
            ("Total".into(), report.total.to_string()),
            ("Accepted".into(), report.accepted.to_string()),
        ];

        // Name the failing check rather than only the count.
        for result in &report.results {
            if result.acceptance_decision == app_lib::domain::vc::AcceptanceDecision::Accept {
                continue;
            }
            let id = if result.credential_id.is_empty() {
                "(no id)".to_string()
            } else {
                result.credential_id.clone()
            };
            let reasons = app_lib::commands::credentials::rejection_reasons(result);
            lines.push((id, reasons.join(", ")));
        }

        if report.revocation_unknown {
            lines.push((
                "Note".into(),
                "no status list supplied — revocation unknown, not absent".into(),
            ));
        }

        let ok = report.accepted == report.total;
        self.verify_result = Some(VerifyOutcome {
            title: kind.to_string(),
            lines,
            ok,
        });

        Ok(format!(
            "{}/{} credential(s) verified",
            report.accepted, report.total
        ))
    }

    fn verify_presentation(&mut self, path: &str, audience: &str) -> Result<String> {
        let raw = read_json_input(path)?;
        let envelope: app_lib::commands::presentation::PresentationEnvelope =
            serde_json::from_str(&raw)?;
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let verdict =
            app_lib::commands::presentation::verify_presentation_impl(conn, &envelope, audience)
                .map_err(|e| anyhow::anyhow!(e))?;

        use app_lib::commands::presentation::PresentationVerification as V;
        let explanation = match verdict {
            V::Accepted => "signature, audience, and nonce all check out",
            V::BadSignature => "signature does not verify",
            V::AudienceMismatch => "not bound to this audience",
            V::Replayed => "already used once",
            V::Malformed => "envelope payload is malformed",
        };
        let ok = verdict == V::Accepted;

        self.verify_result = Some(VerifyOutcome {
            title: "Presentation".into(),
            lines: vec![
                ("Envelope".into(), path.to_string()),
                ("Subject".into(), envelope.subject.clone()),
                ("Audience".into(), audience.to_string()),
                (
                    "Verdict".into(),
                    if ok { "accepted" } else { "rejected" }.to_string(),
                ),
                ("Reason".into(), explanation.to_string()),
            ],
            ok,
        });

        Ok(if ok {
            "Presentation accepted".to_string()
        } else {
            format!("Presentation rejected — {explanation}")
        })
    }

    fn run_migrate(&mut self) {
        let result = (|| -> Result<String> {
            let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
            crate::commands::db::ensure_migration_table(conn)?;
            let before = crate::commands::db::current_version(conn);
            let latest = crate::commands::db::latest_version();
            if before >= latest {
                return Ok(format!("Already at v{before} — nothing to migrate"));
            }
            let applied = crate::commands::db::apply_migrations(conn)?;
            let after = crate::commands::db::current_version(conn);
            Ok(format!(
                "Applied {applied} migration(s) (v{before} → v{after})"
            ))
        })();
        match result {
            Ok(msg) => {
                self.refresh();
                self.toast_ok(msg);
            }
            Err(e) => self.toast_err(format!("{e:#}")),
        }
    }

    fn db_seed(&mut self) -> Result<String> {
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let inserted = crate::commands::db::seed_if_empty(conn)?;
        Ok(if inserted {
            "Seed data inserted".to_string()
        } else {
            "Database already has data — seed skipped".to_string()
        })
    }

    /// Run the same checks `alexandria doctor` runs. Synchronous: the checks
    /// shell out to rustup/java/xcodebuild, so the UI pauses briefly. That is
    /// preferable to a background thread whose result could land after the
    /// user has moved on.
    pub fn run_doctor(&mut self) {
        self.doctor = crate::commands::doctor::collect_sections(&self.ctx, true);
        self.clamp_selection();
    }

    /// Load a page of rows from the selected table into a browsable modal.
    fn open_table_rows(&mut self) {
        let Some((table, _)) = self.db_tables.get(self.selected_index()).cloned() else {
            return;
        };
        let result = self
            .conn()
            .ok_or_else(|| anyhow::anyhow!("vault locked"))
            .and_then(|conn| read_table(conn, &table));
        match result {
            Ok(view) => self.modal = Modal::Rows(view),
            Err(e) => self.toast_err(format!("{e:#}")),
        }
    }

    /// Expand the highlighted row, re-reading it so values are not the ones
    /// the grid had to clip.
    fn open_row_detail(&mut self) {
        let Modal::Rows(view) = &self.modal else {
            return;
        };
        if view.rows.is_empty() {
            return;
        }
        let (table, index) = (view.table.clone(), view.selected);

        let result = self
            .conn()
            .ok_or_else(|| anyhow::anyhow!("vault locked"))
            .and_then(|conn| read_row(conn, &table, index));

        match result {
            Ok(values) => {
                let Modal::Rows(view) = std::mem::replace(&mut self.modal, Modal::None) else {
                    return;
                };
                let pairs = view.columns.iter().cloned().zip(values).collect();
                self.modal = Modal::Row {
                    rows: view,
                    pairs,
                    scroll: 0,
                };
            }
            Err(e) => self.toast_err(format!("{e:#}")),
        }
    }

    /// Return from an expanded row to the grid it came from.
    fn reopen_rows(&mut self) {
        if let Modal::Row { rows, .. } = std::mem::replace(&mut self.modal, Modal::None) {
            self.modal = Modal::Rows(rows);
        }
    }

    /// Copy whatever is currently on screen as JSON.
    ///
    /// One key for every surface, because "copy what I am looking at" is the
    /// same intent whether that is a credential in the list, an expanded
    /// record, or a database row.
    fn copy_current(&mut self) {
        let Some(payload) = self.copy_payload() else {
            return self.toast_err("Nothing here to copy");
        };
        let bytes = payload.len();
        match clipboard::copy(&payload) {
            Ok(method) => {
                let how = method.describe();
                self.toast_ok(format!("{bytes} bytes — {how}"))
            }
            Err(e) => self.toast_err(format!("Copy failed: {e:#}")),
        }
    }

    /// What `y` would copy. Split out so it can be tested without touching the
    /// real clipboard.
    pub(super) fn copy_payload(&self) -> Option<String> {
        match &self.modal {
            Modal::Detail { body, .. } => Some(body.clone()),
            Modal::Row { pairs, .. } => {
                // A row has no JSON of its own, so build an object from its
                // columns rather than copying the rendered layout.
                let map: serde_json::Map<String, serde_json::Value> = pairs
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                serde_json::to_string_pretty(&map).ok()
            }
            _ => match self.tab {
                Tab::Logs => {
                    let shown = logs::entries(self.log_level);
                    if shown.is_empty() {
                        None
                    } else {
                        // Copied as plain text, not JSON: a log is read, not
                        // parsed, and pasting it into an issue should look
                        // like a log.
                        Some(
                            shown
                                .iter()
                                .map(|e| {
                                    format!(
                                        "{} {:<5} {} — {}",
                                        e.time, e.level, e.target, e.message
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    }
                }
                Tab::Credentials => self
                    .selected_credential()
                    .and_then(|vc| serde_json::to_string_pretty(vc).ok()),
                Tab::Roles => self
                    .selected_assessment()
                    .and_then(|ra| serde_json::to_string_pretty(ra).ok()),
                Tab::Organizations => self
                    .selected_organization()
                    .and_then(|org| serde_json::to_string_pretty(org).ok()),
                _ => None,
            },
        }
    }

    fn open_detail(&mut self) {
        let (title, body) = match self.tab {
            Tab::Credentials => match self.selected_credential() {
                Some(vc) => (
                    vc.id.clone().unwrap_or_else(|| "Credential".into()),
                    serde_json::to_string_pretty(vc).unwrap_or_default(),
                ),
                None => return,
            },
            Tab::Roles => match self.selected_assessment() {
                Some(ra) => (
                    ra.role_title.clone(),
                    serde_json::to_string_pretty(ra).unwrap_or_default(),
                ),
                None => return,
            },
            Tab::Organizations => match self.selected_organization() {
                Some(org) => (
                    org.name.clone(),
                    serde_json::to_string_pretty(org).unwrap_or_default(),
                ),
                None => return,
            },
            Tab::Database => return self.open_table_rows(),
            Tab::Verify => match &self.verify_result {
                Some(outcome) => (
                    format!("Last verification — {}", outcome.title),
                    outcome
                        .lines
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                None => return,
            },
            // The log view has no per-row detail; it is already the detail.
            Tab::Logs => return,
            Tab::Doctor => match self.doctor.get(self.selected_index()) {
                Some(section) => (
                    crate::commands::doctor::section_title(section.name).to_string(),
                    section
                        .checks
                        .iter()
                        .map(|c| {
                            format!(
                                "{} {} — {}",
                                if c.ok { "ok " } else { "FAIL" },
                                c.label,
                                c.detail
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                None => return,
            },
        };
        self.modal = Modal::Detail {
            title,
            body,
            scroll: 0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ProfileSelection;
    use crossterm::event::KeyEventKind;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    /// An unlocked app with no vault behind it. Every field the navigation
    /// logic touches is real; only `signer` is absent, so any test that
    /// reaches the backend would fail loudly rather than silently pass.
    fn browsing_app(credentials: usize, assessments: usize) -> App {
        let ctx = ProjectContext {
            root: Some(PathBuf::from("/tmp/project")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
        };
        let mut app = App {
            ctx,
            password_file: None,
            screen: Screen::Browse,
            tab: Tab::Credentials,
            modal: Modal::None,
            toast: None,
            should_quit: false,
            signer: None,
            credentials: Vec::new(),
            assessments: Vec::new(),
            organizations: Vec::new(),
            db_tables: Vec::new(),
            filter: None,
            migration: None,
            verify_result: None,
            doctor: Vec::new(),
            log_level: log::LevelFilter::Debug,
            log_follow: true,
            log_offset: 0,
            hits: Hitboxes::default(),
            list_state: ratatui::widgets::ListState::default(),
            mouse_enabled: false,
            selected: [0; Tab::ALL.len()],
        };
        // The list contents never matter to navigation, only the lengths, so
        // the fixtures stay as cheap as the types allow.
        app.db_tables = (0..credentials)
            .map(|i| (format!("table_{i}"), i as i64))
            .collect();
        app.organizations = (0..assessments)
            .map(|i| Organization {
                id: format!("org{i}"),
                name: format!("Org {i}"),
                owner_address: "addr1".into(),
                did: None,
                created_at: "2026-01-01T00:00:00Z".into(),
            })
            .collect();
        app
    }

    #[test]
    fn tab_cycles_forward_and_backward() {
        let mut app = browsing_app(0, 0);
        assert_eq!(app.tab, Tab::Credentials);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Roles);
        app.on_key(key(KeyCode::BackTab));
        assert_eq!(app.tab, Tab::Credentials);
        // Backward from the first tab wraps to the last rather than
        // underflowing the index.
        app.on_key(key(KeyCode::BackTab));
        assert_eq!(app.tab, *Tab::ALL.last().unwrap());
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Credentials);
    }

    #[test]
    fn selection_wraps_within_the_active_list() {
        let mut app = browsing_app(3, 0);
        app.tab = Tab::Database; // db_tables has 3 entries
        assert_eq!(app.selected_index(), 0);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.selected_index(), 2, "up from the top wraps to the end");
        app.on_key(key(KeyCode::Down));
        assert_eq!(
            app.selected_index(),
            0,
            "down from the end wraps to the top"
        );
    }

    #[test]
    fn selection_is_inert_on_an_empty_list() {
        let mut app = browsing_app(0, 0);
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn selection_is_clamped_when_a_list_shrinks() {
        // A refresh after a deletion must not leave the cursor past the end.
        let mut app = browsing_app(5, 0);
        app.tab = Tab::Database;
        app.selected[Tab::Database.index()] = 4;
        app.db_tables.truncate(2);
        app.clamp_selection();
        assert_eq!(app.selected_index(), 1);
    }

    #[test]
    fn each_tab_keeps_its_own_cursor() {
        let mut app = browsing_app(3, 3);
        app.tab = Tab::Database;
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.selected_index(), 1);
        app.tab = Tab::Organizations;
        assert_eq!(
            app.selected_index(),
            0,
            "a different tab has its own cursor"
        );
        app.tab = Tab::Database;
        assert_eq!(app.selected_index(), 1, "returning restores the cursor");
    }

    #[test]
    fn new_organization_form_blocks_on_missing_required_fields() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        assert!(matches!(app.modal, Modal::Form { .. }));

        // Submitting empty must not dismiss the form or reach the backend
        // (there is no signer here — a backend call would panic or error).
        app.on_key(key(KeyCode::Enter));
        assert!(
            matches!(app.modal, Modal::Form { .. }),
            "form stays open when a required field is empty"
        );
        assert!(app.toast.as_ref().is_some_and(|t| t.is_error));
    }

    #[test]
    fn typing_into_a_form_targets_the_active_field_and_tab_moves_it() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));

        for c in "Acme".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Tab));
        for c in "addr1".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }

        match &app.modal {
            Modal::Form { fields, active, .. } => {
                assert_eq!(fields[0].value, "Acme");
                assert_eq!(fields[1].value, "addr1");
                assert_eq!(*active, 1);
            }
            _ => panic!("expected the form to still be open"),
        }
    }

    #[test]
    fn backspace_edits_only_the_active_field() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        for c in "abc".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Backspace));
        match &app.modal {
            Modal::Form { fields, .. } => {
                assert_eq!(fields[0].value, "ab");
                assert_eq!(fields[1].value, "");
            }
            _ => panic!("expected form"),
        }
    }

    #[test]
    fn escape_closes_a_modal_without_acting() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        app.on_key(key(KeyCode::Esc));
        assert!(matches!(app.modal, Modal::None));
        assert!(
            !app.should_quit,
            "esc in a modal closes it, it does not quit"
        );
    }

    #[test]
    fn ctrl_c_quits_from_anywhere() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        app.on_key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        assert!(app.should_quit);
    }

    #[test]
    fn unlock_screen_masks_and_edits_the_password() {
        let mut app = browsing_app(0, 0);
        app.screen = Screen::Unlock {
            password: String::new(),
            error: None,
        };
        for c in "hunter2".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Backspace));
        match &app.screen {
            Screen::Unlock { password, .. } => assert_eq!(password, "hunter"),
            _ => panic!("expected the unlock screen"),
        }
    }

    #[test]
    fn hints_stay_short_enough_for_a_narrow_terminal() {
        // The footer is one line and gets truncated, so it must fit the
        // narrowest terminal anyone reasonably uses — losing the tail means
        // losing the key that quits.
        let mut app = browsing_app(0, 0);
        for tab in Tab::ALL {
            app.tab = tab;
            let len = app.hints().chars().count();
            assert!(
                len <= 78,
                "hints for {tab:?} are {len} chars, too long for an 80-column terminal"
            );
        }
    }

    #[test]
    fn hints_track_the_active_modal() {
        let mut app = browsing_app(0, 0);
        assert!(app.hints().contains("q quit"));
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        assert!(
            app.hints().contains("submit"),
            "a form advertises its own keys"
        );
    }

    #[test]
    fn help_lists_the_actions_for_the_active_tab() {
        let mut app = browsing_app(0, 0);
        fn has(app: &App, needle: &str) -> bool {
            app.help_lines().iter().any(|(_, d)| d.contains(needle))
        }
        assert!(has(&app, "revoke"), "credentials tab documents revoke");
        app.tab = Tab::Roles;
        assert!(has(&app, "publish"), "roles tab documents publish");
        assert!(!has(&app, "revoke"), "roles tab does not document revoke");
        // The universal keys appear on every tab.
        for tab in Tab::ALL {
            app.tab = tab;
            assert!(has(&app, "quit"));
        }
    }

    /// Build a credential fixture with the fields the filter matches on.
    ///
    /// Constructed through serde rather than by hand: `VerifiableCredential`
    /// has no `Default`, and going through the wire format means the fixture
    /// stays valid if the struct gains fields.
    fn credential(id: &str, issuer: &str, subject: &str, class: &str) -> VerifiableCredential {
        serde_json::from_value(serde_json::json!({
            "@context": ["https://www.w3.org/ns/credentials/v2"],
            "id": id,
            "type": ["VerifiableCredential", class],
            "issuer": issuer,
            "validFrom": "2026-01-01T00:00:00Z",
            "credentialSubject": { "id": subject },
            "proof": {
                "type": "Ed25519Signature2020",
                "created": "2026-01-01T00:00:00Z",
                "verificationMethod": format!("{issuer}#key-1"),
                "proofPurpose": "assertionMethod",
                "jws": "test..signature",
            },
        }))
        .expect("credential fixture")
    }

    fn app_with_credentials() -> App {
        let mut app = browsing_app(0, 0);
        app.credentials = vec![
            credential(
                "urn:uuid:aaa",
                "did:key:issuer1",
                "did:key:alice",
                "SkillCredential",
            ),
            credential(
                "urn:uuid:bbb",
                "did:key:issuer2",
                "did:key:bob",
                "RoleCredential",
            ),
        ];
        app
    }

    #[test]
    fn filter_matches_across_id_issuer_subject_and_type() {
        let mut app = app_with_credentials();
        assert_eq!(app.visible_credentials().len(), 2);

        for (needle, expected_id) in [
            ("aaa", "urn:uuid:aaa"),
            ("issuer2", "urn:uuid:bbb"),
            ("alice", "urn:uuid:aaa"),
            ("rolecredential", "urn:uuid:bbb"),
        ] {
            app.filter = Some(needle.to_string());
            let visible = app.visible_credentials();
            assert_eq!(visible.len(), 1, "filter {needle:?} should match one row");
            assert_eq!(visible[0].id.as_deref(), Some(expected_id));
        }
    }

    #[test]
    fn filter_is_case_insensitive_and_can_match_nothing() {
        let mut app = app_with_credentials();
        app.filter = Some("ALICE".into());
        assert_eq!(app.visible_credentials().len(), 1);
        app.filter = Some("nothing-matches-this".into());
        assert!(app.visible_credentials().is_empty());
    }

    #[test]
    fn selection_follows_the_filtered_list_not_the_full_one() {
        // The cursor indexes the visible rows; a filter that hides the
        // selected row must not leave it pointing past the end.
        let mut app = app_with_credentials();
        app.selected[Tab::Credentials.index()] = 1;
        assert_eq!(
            app.selected_credential().unwrap().id.as_deref(),
            Some("urn:uuid:bbb")
        );

        app.filter = Some("alice".into());
        app.clamp_selection();
        assert_eq!(app.selected_index(), 0);
        assert_eq!(
            app.selected_credential().unwrap().id.as_deref(),
            Some("urn:uuid:aaa"),
            "the cursor now refers to the filtered row"
        );
    }

    #[test]
    fn escape_clears_a_filter_before_it_quits() {
        let mut app = app_with_credentials();
        app.filter = Some("alice".into());
        app.on_key(key(KeyCode::Esc));
        assert!(app.filter.is_none(), "the first esc clears the filter");
        assert!(!app.should_quit, "and does not quit");

        app.on_key(key(KeyCode::Esc));
        assert!(app.should_quit, "with no filter, esc quits");
    }

    #[test]
    fn escape_still_quits_on_tabs_without_a_filter() {
        let mut app = app_with_credentials();
        app.filter = Some("alice".into());
        app.tab = Tab::Database;
        app.on_key(key(KeyCode::Esc));
        assert!(app.should_quit, "the filter belongs to the credentials tab");
    }

    #[test]
    fn slash_prefills_the_form_with_the_active_filter() {
        let mut app = app_with_credentials();
        app.filter = Some("alice".into());
        app.on_key(key(KeyCode::Char('/')));
        match &app.modal {
            Modal::Form { fields, .. } => assert_eq!(fields[0].value, "alice"),
            _ => panic!("expected the filter form"),
        }
    }

    #[test]
    fn verify_tab_offers_both_verifications() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Verify;
        assert_eq!(app.list_len(Tab::Verify), 2);

        // Entry 0 is the bundle check: a path plus an optional time.
        app.on_key(key(KeyCode::Enter));
        match &app.modal {
            Modal::Form { fields, title, .. } => {
                assert!(title.contains("bundle"));
                assert_eq!(fields.len(), 2);
                assert!(fields[0].required);
                assert!(!fields[1].required, "the verification time is optional");
            }
            _ => panic!("expected the bundle form"),
        }

        // Entry 1 is the presentation check, where the audience is required —
        // without it there is nothing to bind against.
        app.modal = Modal::None;
        app.selected[Tab::Verify.index()] = 1;
        app.on_key(key(KeyCode::Enter));
        match &app.modal {
            Modal::Form { fields, title, .. } => {
                assert!(title.contains("presentation") || title.contains("Presentation"));
                assert!(fields.iter().all(|f| f.required));
            }
            _ => panic!("expected the presentation form"),
        }
    }

    #[test]
    fn doctor_runs_on_first_arrival_and_reports_sections() {
        let mut app = browsing_app(0, 0);
        assert!(app.doctor.is_empty());
        // Tab round the bar until Doctor comes up.
        for _ in 0..Tab::ALL.len() {
            app.on_key(key(KeyCode::Tab));
            if app.tab == Tab::Doctor {
                break;
            }
        }
        assert_eq!(app.tab, Tab::Doctor);
        assert!(
            !app.doctor.is_empty(),
            "arriving at the tab runs the checks"
        );
        assert!(app.doctor.iter().any(|s| s.name == "toolchain"));
    }

    #[test]
    fn database_tab_offers_migrate_and_seed() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Database;
        // Seed asks first — it writes rows.
        app.on_key(key(KeyCode::Char('s')));
        assert!(matches!(app.modal, Modal::Confirm { .. }));
        assert!(app.help_lines().iter().any(|(k, _)| *k == "m"));
    }

    // ---- Table row browser ---------------------------------------------

    /// A real SQLite database, so the reader is exercised against the engine
    /// rather than against a mock of it.
    fn fixture_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE creds (
                 id INTEGER PRIMARY KEY,
                 subject TEXT,
                 score REAL,
                 payload BLOB,
                 note TEXT
             );
             INSERT INTO creds VALUES (1, 'did:key:alice', 0.5, x'00112233', 'ok');
             INSERT INTO creds VALUES (2, NULL, NULL, NULL, NULL);
             CREATE TABLE empty_table (a TEXT);
             CREATE TABLE \"order\" (x TEXT);
             INSERT INTO \"order\" VALUES ('keyword-named table');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn read_table_returns_columns_and_formats_every_sqlite_type() {
        let conn = fixture_db();
        let view = read_table(&conn, "creds").unwrap();

        assert_eq!(view.columns, ["id", "subject", "score", "payload", "note"]);
        assert_eq!(view.total, 2);
        assert_eq!(view.rows.len(), 2);

        assert_eq!(view.rows[0][0], "1");
        assert_eq!(view.rows[0][1], "did:key:alice");
        assert_eq!(view.rows[0][2], "0.5");
        // Binary is described, never dumped — raw bytes corrupt the display.
        assert_eq!(view.rows[0][3], "<blob 4 bytes>");

        // NULL is spelled out rather than shown as an empty cell, which would
        // be indistinguishable from an empty string.
        assert_eq!(view.rows[1][1], "NULL");
        assert_eq!(view.rows[1][3], "NULL");
    }

    #[test]
    fn read_table_handles_an_empty_table_and_a_keyword_name() {
        let conn = fixture_db();

        let empty = read_table(&conn, "empty_table").unwrap();
        assert_eq!(empty.total, 0);
        assert!(empty.rows.is_empty());
        assert_eq!(empty.columns, ["a"], "columns are known even with no rows");

        // `order` is a reserved word; the query quotes table names for exactly
        // this reason.
        let kw = read_table(&conn, "order").unwrap();
        assert_eq!(kw.rows[0][0], "keyword-named table");
    }

    #[test]
    fn read_table_reports_a_missing_table_rather_than_panicking() {
        let conn = fixture_db();
        assert!(read_table(&conn, "no_such_table").is_err());
    }

    #[test]
    fn cells_are_flattened_and_clipped() {
        // Newlines would break the row grid apart, so they become a marker.
        assert_eq!(clip("a\nb\tc", MAX_CELL), "a⏎b⏎c");

        let long = "x".repeat(MAX_CELL + 50);
        let clipped = clip(&long, MAX_CELL);
        assert_eq!(
            clipped.chars().count(),
            MAX_CELL + 1,
            "kept MAX_CELL plus the ellipsis"
        );
        assert!(clipped.ends_with('…'));

        // Multi-byte content must be clipped by characters, not bytes.
        let wide = "é".repeat(MAX_CELL + 10);
        assert_eq!(clip(&wide, MAX_CELL).chars().count(), MAX_CELL + 1);
    }

    fn rows_app(rows: usize, cols: usize) -> App {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Database;
        app.modal = Modal::Rows(TableRows {
            table: "t".into(),
            columns: (0..cols).map(|i| format!("c{i}")).collect(),
            rows: (0..rows)
                .map(|r| (0..cols).map(|c| format!("{r}-{c}")).collect())
                .collect(),
            total: rows as i64,
            selected: 0,
            col_offset: 0,
        });
        app
    }

    #[test]
    fn row_cursor_clamps_at_both_ends() {
        let mut app = rows_app(3, 2);
        app.on_key(key(KeyCode::Up));
        match &app.modal {
            Modal::Rows(v) => assert_eq!(v.selected, 0, "cannot move above the first row"),
            _ => panic!("expected the row browser"),
        }
        for _ in 0..10 {
            app.on_key(key(KeyCode::Down));
        }
        match &app.modal {
            Modal::Rows(v) => assert_eq!(v.selected, 2, "stops at the last row"),
            _ => panic!("expected the row browser"),
        }
    }

    #[test]
    fn column_scrolling_always_leaves_one_column() {
        let mut app = rows_app(1, 3);
        for _ in 0..10 {
            app.on_key(key(KeyCode::Right));
        }
        match &app.modal {
            Modal::Rows(v) => assert_eq!(v.col_offset, 2, "never scrolls past the last column"),
            _ => panic!("expected the row browser"),
        }
        for _ in 0..10 {
            app.on_key(key(KeyCode::Left));
        }
        match &app.modal {
            Modal::Rows(v) => assert_eq!(v.col_offset, 0),
            _ => panic!("expected the row browser"),
        }
    }

    #[test]
    fn read_row_returns_full_values_the_grid_had_to_clip() {
        let conn = Connection::open_in_memory().unwrap();
        let long = "x".repeat(MAX_CELL + 500);
        conn.execute_batch("CREATE TABLE t (a TEXT, b BLOB);")
            .unwrap();
        conn.execute("INSERT INTO t VALUES (?1, x'0011')", [&long])
            .unwrap();

        // The grid clips to keep cells laid out...
        let grid = read_table(&conn, "t").unwrap();
        assert_eq!(grid.rows[0][0].chars().count(), MAX_CELL + 1);

        // ...the expanded row is the whole point of not clipping that hard.
        let row = read_row(&conn, "t", 0).unwrap();
        assert_eq!(row[0].chars().count(), long.chars().count());
        assert!(row[0].chars().count() > MAX_CELL);
        // Blobs stay described even here — the cap is about text, not binary.
        assert_eq!(row[1], "<blob 2 bytes>");
    }

    #[test]
    fn read_row_picks_the_row_at_that_position() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (v TEXT);
             INSERT INTO t VALUES ('first'), ('second'), ('third');",
        )
        .unwrap();

        // Position must agree with what the grid put at that index, or the
        // reader opens a different row than the one highlighted.
        let grid = read_table(&conn, "t").unwrap();
        for (i, expected) in grid.rows.iter().enumerate() {
            assert_eq!(read_row(&conn, "t", i).unwrap()[0], expected[0]);
        }
        assert!(
            read_row(&conn, "t", 99).is_err(),
            "past the end is an error"
        );
    }

    #[test]
    fn enter_expands_the_selected_row_and_escape_returns_to_it() {
        // Without a signer there is no connection, so this exercises the state
        // machine: the grid must survive the round trip either way.
        let mut app = rows_app(5, 3);
        if let Modal::Rows(v) = &mut app.modal {
            v.selected = 3;
            v.col_offset = 1;
        }

        // Hand-build the expanded modal the way open_row_detail would, since
        // the read itself needs a database.
        let Modal::Rows(view) = std::mem::replace(&mut app.modal, Modal::None) else {
            panic!("expected the grid")
        };
        let pairs = vec![("c0".to_string(), "3-0".to_string())];
        app.modal = Modal::Row {
            rows: view,
            pairs,
            scroll: 0,
        };

        app.on_key(key(KeyCode::Down));
        match &app.modal {
            Modal::Row { scroll, .. } => assert_eq!(*scroll, 1, "the expanded row scrolls"),
            _ => panic!("expected the expanded row"),
        }

        app.on_key(key(KeyCode::Esc));
        match &app.modal {
            Modal::Rows(v) => {
                assert_eq!(v.selected, 3, "cursor restored");
                assert_eq!(v.col_offset, 1, "column scroll restored");
            }
            _ => panic!("esc should return to the grid, not close it"),
        }
        assert!(!app.should_quit);
    }

    // ---- Mouse ---------------------------------------------------------

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn the_mouse_is_off_until_asked_for() {
        // Enabling mouse reporting takes drag-to-select away from the
        // terminal. That is not a cost to impose by default on a tool people
        // open when something has gone wrong and they want to copy the error.
        let mut app = browsing_app(0, 0);
        assert!(!app.mouse_enabled);

        // And while off, events are ignored rather than acted on.
        app.tab = Tab::Database;
        app.db_tables = vec![("a".into(), 0), ("b".into(), 0)];
        app.hits.list = Some(ratatui::layout::Rect::new(0, 4, 40, 10));
        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 5));
        assert_eq!(app.selected_index(), 0, "a click must do nothing while off");
    }

    #[test]
    fn capital_m_toggles_the_mouse_and_says_what_changed() {
        let mut app = browsing_app(0, 0);
        app.on_key(key(KeyCode::Char('M')));
        assert!(app.mouse_enabled);
        // The toast has to name the escape hatch, or losing selection looks
        // like a bug rather than a mode.
        let toast = app.toast.as_ref().expect("a toast");
        assert!(toast.text.contains("select"), "got: {}", toast.text);

        app.on_key(key(KeyCode::Char('M')));
        assert!(!app.mouse_enabled);
    }

    #[test]
    fn lowercase_m_keeps_its_existing_meaning() {
        // `m` is import on Credentials and migrate on Database. Rebinding it
        // would have made one key mean three things.
        let mut app = app_with_credentials();
        app.on_key(key(KeyCode::Char('m')));
        assert!(!app.mouse_enabled, "m must not toggle the mouse");
        assert!(matches!(app.modal, Modal::Form { .. }), "m still imports");
    }

    #[test]
    fn clicking_the_tab_bar_selects_that_tab() {
        let mut app = browsing_app(0, 0);
        app.mouse_enabled = true;
        app.hits.tab_row = 1;
        app.hits.tabs = vec![(2, 15, Tab::Credentials), (18, 26, Tab::Logs)];

        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 20, 1));
        assert_eq!(app.tab, Tab::Logs);

        // A click in the gap between labels selects nothing rather than the
        // nearest tab.
        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 16, 1));
        assert_eq!(app.tab, Tab::Logs);
    }

    #[test]
    fn a_click_aims_and_a_second_click_acts() {
        // One click selects, a second on the same row opens it — so a
        // misplaced click can never trigger an action.
        let mut app = browsing_app(4, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Database;
        app.hits.list = Some(ratatui::layout::Rect::new(1, 5, 40, 10));
        app.hits.list_offset = 0;

        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 7));
        assert_eq!(app.selected_index(), 2, "row under the cursor is selected");
        assert!(matches!(app.modal, Modal::None), "one click must not open");
    }

    #[test]
    fn clicks_account_for_the_scroll_offset() {
        // Without this the wrong row is acted on the moment a list scrolls,
        // which is a silent mistake rather than a visible one.
        let mut app = browsing_app(50, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Database;
        app.hits.list = Some(ratatui::layout::Rect::new(1, 5, 40, 10));
        app.hits.list_offset = 20;

        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 8));
        assert_eq!(app.selected_index(), 23, "offset 20 + 3 rows down");
    }

    #[test]
    fn clicking_past_the_last_row_does_nothing() {
        let mut app = browsing_app(2, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Database;
        app.hits.list = Some(ratatui::layout::Rect::new(1, 5, 40, 10));

        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 12));
        assert_eq!(app.selected_index(), 0, "empty space below the rows");
    }

    #[test]
    fn a_modal_swallows_clicks_meant_for_it() {
        // Clicking through a modal would act on a list the user cannot see.
        let mut app = browsing_app(4, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Database;
        app.hits.list = Some(ratatui::layout::Rect::new(1, 5, 40, 10));
        app.modal = Modal::Help;

        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 7));
        assert_eq!(app.selected_index(), 0);
        assert!(matches!(app.modal, Modal::Help), "the modal stays put");
    }

    #[test]
    fn the_wheel_scrolls_whatever_is_in_front() {
        let mut app = browsing_app(5, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Database;

        app.on_mouse(mouse(MouseEventKind::ScrollDown, 5, 7));
        assert_eq!(app.selected_index(), 1);
        app.on_mouse(mouse(MouseEventKind::ScrollUp, 5, 7));
        assert_eq!(app.selected_index(), 0);

        // With a record open, the wheel scrolls the record instead.
        app.modal = Modal::Detail {
            title: "t".into(),
            body: "body".into(),
            scroll: 0,
        };
        app.on_mouse(mouse(MouseEventKind::ScrollDown, 5, 7));
        match &app.modal {
            Modal::Detail { scroll, .. } => assert_eq!(*scroll, 1),
            _ => panic!("expected the detail view"),
        }
        assert_eq!(app.selected_index(), 0, "the list behind must not move");
    }

    #[test]
    fn the_wheel_releases_log_follow_like_the_keyboard_does() {
        let mut app = browsing_app(0, 0);
        app.mouse_enabled = true;
        app.tab = Tab::Logs;
        assert!(app.log_follow);
        app.on_mouse(mouse(MouseEventKind::ScrollUp, 5, 7));
        assert!(!app.log_follow);
    }

    #[test]
    fn clicks_are_ignored_on_the_unlock_screen() {
        // Nothing there is clickable, and a stray click must not disturb a
        // half-typed password.
        let mut app = browsing_app(0, 0);
        app.mouse_enabled = true;
        app.screen = Screen::Unlock {
            password: "half-typed".into(),
            error: None,
        };
        app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 7));
        match &app.screen {
            Screen::Unlock { password, .. } => assert_eq!(password, "half-typed"),
            _ => panic!("still unlocking"),
        }
    }

    // ---- Tab shortcuts -------------------------------------------------

    #[test]
    fn digits_jump_straight_to_a_tab() {
        let mut app = browsing_app(0, 0);
        for (digit, expected) in [
            ('1', Tab::Credentials),
            ('4', Tab::Database),
            ('7', Tab::Logs),
            ('2', Tab::Roles),
        ] {
            app.on_key(key(KeyCode::Char(digit)));
            assert_eq!(app.tab, expected, "`{digit}` should select {expected:?}");
        }
    }

    #[test]
    fn digits_outside_the_tab_range_do_nothing() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Roles;
        // There is no tab 0, and no tab 8.
        app.on_key(key(KeyCode::Char('0')));
        assert_eq!(app.tab, Tab::Roles);
        app.on_key(key(KeyCode::Char('8')));
        assert_eq!(app.tab, Tab::Roles);
        app.on_key(key(KeyCode::Char('9')));
        assert_eq!(app.tab, Tab::Roles);
    }

    #[test]
    fn a_digit_typed_into_a_form_stays_in_the_form() {
        // The shortcut must not steal digits from input. A date typed into
        // "Verify as of" would otherwise teleport the user to another tab
        // mid-entry and drop what they had typed.
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));

        for c in "2026-01-01".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }

        assert_eq!(app.tab, Tab::Organizations, "the form kept the digits");
        match &app.modal {
            Modal::Form { fields, .. } => assert_eq!(fields[0].value, "2026-01-01"),
            _ => panic!("the form must still be open"),
        }
    }

    #[test]
    fn jumping_to_doctor_runs_its_checks() {
        // The shortcut must do everything tabbing there does, not just move
        // the index.
        let mut app = browsing_app(0, 0);
        assert!(app.doctor.is_empty());
        app.on_key(key(KeyCode::Char('6')));
        assert_eq!(app.tab, Tab::Doctor);
        assert!(!app.doctor.is_empty(), "arriving must run the checks");
    }

    // ---- Logs ----------------------------------------------------------

    #[test]
    fn the_log_tab_advertises_its_own_keys() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Logs;
        let hints = app.hints();
        // Enter does nothing on this tab, so offering it would point at
        // nothing.
        assert!(!hints.contains("⏎ detail"), "got: {hints}");
        for key in ["f follow", "l level", "c clear"] {
            assert!(hints.contains(key), "missing {key} in: {hints}");
        }
        assert!(hints.chars().count() <= 78, "footer is truncated: {hints}");
    }

    #[test]
    fn scrolling_the_log_turns_off_follow() {
        // Otherwise the next record yanks the view back to the bottom and the
        // line being read disappears.
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Logs;
        assert!(app.log_follow, "follow is the sensible default");

        app.on_key(key(KeyCode::Up));
        assert!(!app.log_follow, "scrolling hands control to the reader");

        // End gives it back.
        app.on_key(key(KeyCode::End));
        assert!(app.log_follow);

        // And f toggles explicitly.
        app.on_key(key(KeyCode::Char('f')));
        assert!(!app.log_follow);
    }

    #[test]
    fn the_level_filter_cycles_through_every_level() {
        use log::LevelFilter::*;
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Logs;
        app.log_level = Error;

        let mut seen = vec![app.log_level];
        for _ in 0..4 {
            app.on_key(key(KeyCode::Char('l')));
            seen.push(app.log_level);
        }
        // Cycles Error → Warn → Info → Debug → back to Error, so no level is
        // unreachable.
        assert_eq!(seen, vec![Error, Warn, Info, Debug, Error]);
    }

    #[test]
    fn clearing_the_log_empties_it() {
        let _guard = logs::test_lock();
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Logs;
        logs::clear();
        log::info!("something happened");
        // The global logger may not be installed in a test process, so only
        // assert the clear path when capture is actually active.
        if logs::len() > 0 {
            app.on_key(key(KeyCode::Char('c')));
            assert_eq!(logs::len(), 0);
        }
    }

    #[test]
    fn copying_the_log_yields_plain_text_not_json() {
        let _guard = logs::test_lock();
        logs::clear();
        logs::install();
        log::error!("action failed: Incorrect vault password");

        let mut app = browsing_app(0, 0);
        app.tab = Tab::Logs;
        if let Some(payload) = app.copy_payload() {
            // A log is read, not parsed — pasting it into an issue should look
            // like a log.
            assert!(
                payload.contains("Incorrect vault password"),
                "got: {payload}"
            );
            assert!(payload.contains("ERROR"), "got: {payload}");
            assert!(serde_json::from_str::<serde_json::Value>(&payload).is_err());
        }
        logs::clear();
    }

    // ---- Copying -------------------------------------------------------

    #[test]
    fn copying_a_credential_yields_its_json() {
        let mut app = app_with_credentials();
        let payload = app.copy_payload().expect("a credential is selected");

        // Must be the credential document, parseable by whoever it is pasted
        // into — not the rendered summary.
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        assert_eq!(parsed["id"], "urn:uuid:aaa");
        assert_eq!(parsed["issuer"], "did:key:issuer1");
        assert!(
            parsed["proof"].is_object(),
            "the proof must survive the copy"
        );

        // Follows the cursor rather than always copying the first row.
        app.selected[Tab::Credentials.index()] = 1;
        let second = app.copy_payload().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(parsed["id"], "urn:uuid:bbb");
    }

    #[test]
    fn copying_respects_the_filter() {
        // The cursor indexes visible rows, so a filtered list must copy the
        // row the user is actually looking at.
        let mut app = app_with_credentials();
        app.filter = Some("bob".into());
        app.clamp_selection();
        let parsed: serde_json::Value = serde_json::from_str(&app.copy_payload().unwrap()).unwrap();
        assert_eq!(parsed["id"], "urn:uuid:bbb");
    }

    #[test]
    fn copying_a_database_row_builds_an_object_from_its_columns() {
        // A row has no document of its own; copying the rendered layout would
        // paste ASCII art into whatever received it.
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Database;
        app.modal = Modal::Row {
            rows: TableRows {
                table: "creds".into(),
                columns: vec!["id".into(), "subject".into()],
                rows: vec![vec!["1".into(), "did:key:alice".into()]],
                total: 1,
                selected: 0,
                col_offset: 0,
            },
            pairs: vec![
                ("id".into(), "1".into()),
                ("subject".into(), "did:key:alice".into()),
            ],
            scroll: 0,
        };

        let parsed: serde_json::Value = serde_json::from_str(&app.copy_payload().unwrap()).unwrap();
        assert_eq!(parsed["id"], "1");
        assert_eq!(parsed["subject"], "did:key:alice");
    }

    #[test]
    fn copying_an_open_detail_copies_what_is_shown() {
        let mut app = app_with_credentials();
        app.modal = Modal::Detail {
            title: "t".into(),
            body: "{\"shown\": true}".into(),
            scroll: 0,
        };
        assert_eq!(app.copy_payload().unwrap(), "{\"shown\": true}");
    }

    #[test]
    fn there_is_nothing_to_copy_on_an_empty_list() {
        let app = browsing_app(0, 0);
        assert!(app.copy_payload().is_none());
    }

    // ---- Pasting -------------------------------------------------------

    #[test]
    fn json_input_accepts_a_pasted_document_or_a_path() {
        // A document, used as-is.
        let doc = r#"{"id":"urn:uuid:abc"}"#;
        assert_eq!(read_json_input(doc).unwrap(), doc);
        // Leading whitespace from a paste must not defeat the check.
        assert_eq!(read_json_input(&format!("\n  {doc}  ")).unwrap(), doc);
        // An array is a document too — bundles are sent both ways.
        assert_eq!(read_json_input("[1,2]").unwrap(), "[1,2]");

        // Anything else is a path, and a missing one says so rather than
        // being silently treated as JSON.
        let err = read_json_input("/nope/missing.json")
            .unwrap_err()
            .to_string();
        assert!(err.contains("/nope/missing.json"), "got: {err}");

        // A real file still works.
        let path = std::env::temp_dir().join("alexandria-json-input-test.json");
        std::fs::write(&path, doc).unwrap();
        assert_eq!(read_json_input(path.to_str().unwrap()).unwrap(), doc);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn paste_lands_in_the_active_field_without_submitting() {
        let mut app = browsing_app(0, 0);
        app.tab = Tab::Organizations;
        app.on_key(key(KeyCode::Char('n')));
        app.on_key(key(KeyCode::Tab)); // move to the second field

        // Multi-line, as a real credential paste would be.
        app.on_paste("{\n  \"a\": 1\n}");

        match &app.modal {
            Modal::Form { fields, active, .. } => {
                assert_eq!(*active, 1, "paste must not move the cursor");
                assert!(fields[0].value.is_empty(), "went to the wrong field");
                assert!(fields[1].value.contains("\"a\""));
                assert!(
                    fields[1].value.contains('\n'),
                    "newlines are kept — the value is JSON, not a line of text"
                );
            }
            _ => panic!("the form must still be open — a paste is not a submit"),
        }
    }

    #[test]
    fn pasting_a_password_drops_a_trailing_newline() {
        // Clipboards routinely carry one, and it would otherwise be fed
        // straight into the key derivation and fail the unlock.
        let mut app = browsing_app(0, 0);
        app.screen = Screen::Unlock {
            password: String::new(),
            error: None,
        };
        app.on_paste("hunter2\n");
        match &app.screen {
            Screen::Unlock { password, .. } => assert_eq!(password, "hunter2"),
            _ => panic!("expected the unlock screen"),
        }
    }

    #[test]
    fn escape_closes_the_row_browser() {
        let mut app = rows_app(2, 2);
        app.on_key(key(KeyCode::Esc));
        assert!(matches!(app.modal, Modal::None));
        assert!(!app.should_quit, "closing the browser does not quit");
    }

    #[test]
    fn question_mark_opens_and_closes_help() {
        let mut app = browsing_app(0, 0);
        app.on_key(key(KeyCode::Char('?')));
        assert!(matches!(app.modal, Modal::Help));
        app.on_key(key(KeyCode::Esc));
        assert!(matches!(app.modal, Modal::None));
        assert!(!app.should_quit, "closing help does not quit");
    }
}

/// Read a page of rows from `table`.
///
/// Free-standing rather than a method so it can be exercised against a real
/// SQLite database in tests, without a vault or an unlocked keystore.
/// Render one SQLite value as display text.
///
/// `limit` differs between the grid and the expanded row: the grid needs cells
/// short enough to lay out, the expanded row exists precisely to show what the
/// grid cut.
fn cell_text(row: &rusqlite::Row, i: usize, limit: usize) -> rusqlite::Result<String> {
    use rusqlite::types::ValueRef;

    let text = match row.get_ref(i)? {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
        // Never dump binary into a terminal: it corrupts the display and tells
        // the reader nothing.
        ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
    };
    Ok(clip(&text, limit))
}

/// Read every column of one row, without the grid's narrow cell cap.
///
/// Re-queried rather than taken from the loaded page, so the values are the
/// full ones. `LIMIT 1 OFFSET n` against the same unordered `SELECT *` returns
/// the same row the page put at position `n`: identical statement, unchanged
/// table.
pub(super) fn read_row(conn: &Connection, table: &str, index: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" LIMIT 1 OFFSET {index}"))?;
    let count = stmt.column_count();
    let mut rows = stmt.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| anyhow::anyhow!("row {} is no longer there", index + 1))?;
    Ok((0..count)
        .map(|i| cell_text(row, i, MAX_DETAIL_CELL))
        .collect::<rusqlite::Result<Vec<String>>>()?)
}

pub(super) fn read_table(conn: &Connection, table: &str) -> Result<TableRows> {
    // Table names come from sqlite_master, not from user input, so
    // interpolating one here cannot inject. Quoted anyway, because plenty
    // of them would otherwise collide with SQL keywords.
    let total: i64 = conn
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
            r.get(0)
        })
        .unwrap_or(-1);

    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" LIMIT {ROW_PAGE}"))?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let column_count = columns.len();

    let rows = stmt
        .query_map([], |row| {
            (0..column_count)
                .map(|i| cell_text(row, i, MAX_CELL))
                .collect::<rusqlite::Result<Vec<String>>>()
        })?
        .collect::<rusqlite::Result<Vec<Vec<String>>>>()?;

    Ok(TableRows {
        table: table.to_string(),
        columns,
        rows,
        total,
        selected: 0,
        col_offset: 0,
    })
}
