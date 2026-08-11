//! TUI state and the actions it can take.
//!
//! Every mutation goes through the same `app_lib::commands::*` impl functions
//! the subcommands use — this module decides *what* to call and *when*, never
//! *how* a credential is signed or stored.

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::Connection;

use app_lib::commands::role_assessment::{Organization, RoleAssessment};
use app_lib::domain::vc::VerifiableCredential;

use crate::context::ProjectContext;
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
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Credentials,
        Tab::Roles,
        Tab::Organizations,
        Tab::Database,
        Tab::Verify,
        Tab::Doctor,
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
        }
    }

    /// Whether the tab bar shows a row count for this tab. Verify is a menu
    /// and Doctor is a report, so a count would be noise.
    pub fn shows_count(self) -> bool {
        !matches!(self, Tab::Verify | Tab::Doctor)
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
        "Credential bundle (offline)",
        "Check a §20.4 survivability bundle with no infrastructure",
    ),
    (
        "Presentation",
        "Check a presentation envelope against an expected audience",
    ),
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
}

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
                self.signer = Some(signer);
                self.screen = Screen::Browse;
                self.refresh();
            }
            Err(e) => {
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
            Modal::None => "↑↓ move · ⏎ detail · ? keys · q quit",
            Modal::Confirm { .. } => "y confirm · n/esc cancel",
            Modal::Form { .. } => "⇥ field · ⏎ submit · esc cancel",
            Modal::Detail { .. } => "↑↓ scroll · esc close",
            Modal::Help => "esc close",
        }
    }

    /// The full key reference for the active tab, shown by `?`.
    pub fn help_lines(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![
            ("↑ ↓ / j k", "move the selection"),
            ("⇥ / ⇧⇥", "switch tab"),
            ("⏎", "open the full record"),
            ("g", "refresh from the database"),
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
                ("m", "run pending migrations"),
                ("s", "seed demo data if the database is empty"),
            ],
            Tab::Verify => vec![("⏎", "run the selected verification")],
            Tab::Doctor => vec![("g", "re-run the checks")],
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
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Char('g') => {
                if self.tab == Tab::Doctor {
                    self.run_doctor();
                    self.toast_ok("Checks re-run");
                } else {
                    self.refresh();
                    self.toast_ok("Refreshed");
                }
            }
            KeyCode::Char('?') => self.modal = Modal::Help,
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
                    fields: vec![Field::new("File path", true)],
                    active: 0,
                    action: Pending::ImportFile,
                };
            }
            KeyCode::Char('n') => {
                self.modal = Modal::Form {
                    title: "Issue credential".into(),
                    fields: vec![Field::new("Request JSON path", true)],
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
                        Field::new("Bundle JSON path", true),
                        Field::new("At (ISO 8601, optional)", false),
                    ],
                    active: 0,
                    action: Pending::VerifyBundle,
                };
            }
            _ => {
                self.modal = Modal::Form {
                    title: "Verify presentation".into(),
                    fields: vec![
                        Field::new("Envelope JSON path", true),
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
                fields: vec![Field::new("Request JSON path", true)],
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
            Modal::Detail { scroll, .. } => match key.code {
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
                self.refresh();
                self.toast_ok(msg);
            }
            Err(e) => self.toast_err(format!("{e:#}")),
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
        let payload = std::fs::read_to_string(path)?;
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
        let raw = std::fs::read_to_string(request_path)?;
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
        let raw = std::fs::read_to_string(request_path)?;
        let req: app_lib::commands::role_assessment::CreateRoleAssessmentRequest =
            serde_json::from_str(&raw)?;
        let now = vault::now_rfc3339();
        let conn = self.conn().ok_or_else(|| anyhow::anyhow!("vault locked"))?;
        let ra = app_lib::commands::role_assessment::create_role_assessment_impl(conn, &req, &now)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(format!("Created {}", ra.role_title))
    }

    fn verify_bundle(&mut self, path: &str, at: Option<&str>) -> Result<String> {
        let json = std::fs::read_to_string(path)?;
        let now = at.map(str::to_string).unwrap_or_else(vault::now_rfc3339);
        let (accepted, total) =
            app_lib::commands::credentials::verify_bundle_offline_impl(&json, &now)
                .map_err(|e| anyhow::anyhow!(e))?;

        let ok = accepted == total;
        self.verify_result = Some(VerifyOutcome {
            title: "Credential bundle".into(),
            lines: vec![
                ("Bundle".into(), path.to_string()),
                ("At".into(), now),
                ("Total".into(), total.to_string()),
                ("Accepted".into(), accepted.to_string()),
            ],
            ok,
        });

        // A bundle that fails to verify is a result, not an error — the
        // outcome pane reports it and the app carries on.
        Ok(format!("{accepted}/{total} credential(s) verified"))
    }

    fn verify_presentation(&mut self, path: &str, audience: &str) -> Result<String> {
        let raw = std::fs::read_to_string(path)?;
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
            Tab::Database => return,
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
            root: PathBuf::from("/tmp/project"),
            tauri_dir: PathBuf::from("/tmp/project/src-tauri"),
            app_data_dir: PathBuf::from("/tmp/appdata"),
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
