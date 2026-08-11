//! Rendering. Pure: reads [`App`] and draws, never mutates or calls the
//! backend.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use super::app::{App, Modal, Screen, Tab};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Unlock { .. } => draw_unlock(frame, app),
        Screen::Browse => draw_browse(frame, app),
    }
}

// ---- Unlock -------------------------------------------------------------

fn draw_unlock(frame: &mut Frame, app: &App) {
    let Screen::Unlock { password, error } = &app.screen else {
        return;
    };

    let area = centered(frame.area(), 60, 9);
    frame.render_widget(Clear, area);

    let masked = "•".repeat(password.chars().count());
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Password  ", Style::default().fg(MUTED)),
            Span::styled(masked, Style::default().fg(ACCENT)),
            Span::styled("▌", Style::default().fg(ACCENT)),
        ]),
        Line::from(""),
    ];
    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  ⏎ unlock · esc quit",
        Style::default().fg(MUTED),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" ⬡ Alexandria — unlock vault ");

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

// ---- Browse -------------------------------------------------------------

fn draw_browse(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tabs
            Constraint::Min(3),    // body
            Constraint::Length(1), // status
        ])
        .split(frame.area());

    draw_tabs(frame, app, chunks[0]);
    draw_body(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);

    match &app.modal {
        Modal::None => {}
        Modal::Confirm { title, body, .. } => draw_confirm(frame, title, body),
        Modal::Form {
            title,
            fields,
            active,
            ..
        } => draw_form(frame, title, fields, *active),
        Modal::Detail {
            title,
            body,
            scroll,
        } => draw_detail(frame, title, body, *scroll),
        Modal::Help => draw_help(frame, app),
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            if !t.shows_count() {
                return Line::from(t.title());
            }
            let count = match t {
                Tab::Credentials => app.visible_credentials().len(),
                Tab::Roles => app.assessments.len(),
                Tab::Organizations => app.organizations.len(),
                _ => 0,
            };
            Line::from(format!("{} ({})", t.title(), count))
        })
        .collect();

    let index = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(index)
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MUTED))
                .title(" ⬡ Alexandria "),
        );
    frame.render_widget(tabs, area);
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_list(frame, app, columns[0]);
    draw_detail_pane(frame, app, columns[1]);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = match app.tab {
        Tab::Credentials => app
            .visible_credentials()
            .into_iter()
            .map(|vc| {
                let class = vc
                    .type_
                    .iter()
                    .find(|t| t.as_str() != "VerifiableCredential")
                    .map(|s| s.as_str())
                    .unwrap_or("Credential");
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{class:<22}"), Style::default().fg(ACCENT)),
                    Span::styled(
                        vc.id.as_deref().unwrap_or("(no id)").to_string(),
                        Style::default().fg(MUTED),
                    ),
                ]))
            })
            .collect(),
        Tab::Roles => app
            .assessments
            .iter()
            .map(|ra| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<28}", ra.role_title), Style::default()),
                    Span::styled(ra.status.clone(), status_style(&ra.status)),
                ]))
            })
            .collect(),
        Tab::Organizations => app
            .organizations
            .iter()
            .map(|org| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<28}", org.name), Style::default()),
                    Span::styled(org.owner_address.clone(), Style::default().fg(MUTED)),
                ]))
            })
            .collect(),
        Tab::Database => app
            .db_tables
            .iter()
            .map(|(name, count)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{name:<34}"), Style::default()),
                    Span::styled(
                        if *count < 0 {
                            "?".to_string()
                        } else {
                            count.to_string()
                        },
                        Style::default().fg(MUTED),
                    ),
                ]))
            })
            .collect(),
        Tab::Verify => crate::tui::app::VERIFY_ENTRIES
            .iter()
            .map(|(name, _)| ListItem::new(Line::from(Span::raw(*name))))
            .collect(),
        Tab::Doctor => app
            .doctor
            .iter()
            .map(|section| {
                let ok = section.ok();
                let failed = section.checks.iter().filter(|c| !c.ok).count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(
                            "{:<24}",
                            crate::commands::doctor::section_title(section.name)
                        ),
                        Style::default(),
                    ),
                    Span::styled(
                        if ok {
                            "ok".to_string()
                        } else {
                            format!("{failed} failed")
                        },
                        Style::default().fg(if ok { Color::Green } else { Color::Red }),
                    ),
                ]))
            })
            .collect(),
    };

    let empty = items.is_empty();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MUTED))
                .title(format!(" {} ", app.tab.title())),
        )
        .highlight_style(
            Style::default()
                .bg(ACCENT)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▌");

    if empty {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(MUTED))
            .title(format!(" {} ", app.tab.title()));
        frame.render_widget(
            Paragraph::new("  nothing here yet")
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default();
    state.select(Some(app.selected_index()));
    frame.render_stateful_widget(list, area, &mut state);
}

fn status_style(status: &str) -> Style {
    match status {
        "published" => Style::default().fg(Color::Green),
        "archived" => Style::default().fg(MUTED),
        _ => Style::default().fg(Color::Yellow),
    }
}

fn draw_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = match app.tab {
        Tab::Credentials => match app.selected_credential() {
            Some(vc) => {
                let mut l = vec![
                    kv("ID", vc.id.as_deref().unwrap_or("(no envelope id)")),
                    kv("Issuer", vc.issuer.as_str()),
                    kv("Subject", vc.credential_subject.id.as_str()),
                    kv("Issued", &vc.valid_from),
                ];
                if let Some(exp) = &vc.valid_until {
                    l.push(kv("Expires", exp));
                }
                l.push(Line::from(""));
                l.push(Line::from(Span::styled(
                    "  ⏎ for the full JSON",
                    Style::default().fg(MUTED),
                )));
                l
            }
            None => vec![],
        },
        Tab::Roles => match app.selected_assessment() {
            Some(ra) => {
                let mut l = vec![
                    kv("ID", &ra.id),
                    kv("Role", &ra.role_title),
                    kv("Org", &ra.org_id),
                    kv("Status", &ra.status),
                ];
                if let Some(course) = &ra.course_id {
                    l.push(kv("Course", course));
                }
                if !ra.skill_ids.is_empty() {
                    l.push(kv("Skills", &ra.skill_ids.join(", ")));
                }
                if let Some(level) = &ra.required_assurance_level {
                    l.push(kv("Assurance", level));
                }
                l.push(kv("Updated", &ra.updated_at));
                l
            }
            None => vec![],
        },
        Tab::Organizations => match app.selected_organization() {
            Some(org) => {
                let mut l = vec![
                    kv("ID", &org.id),
                    kv("Name", &org.name),
                    kv("Owner", &org.owner_address),
                ];
                if let Some(did) = &org.did {
                    l.push(kv("DID", did));
                }
                l.push(kv("Created", &org.created_at));
                l
            }
            None => vec![],
        },
        Tab::Database => {
            let mut l = vec![
                kv("Path", &app.ctx.db_path().display().to_string()),
                kv("Tables", &app.db_tables.len().to_string()),
                kv(
                    "Rows",
                    &app.db_tables
                        .iter()
                        .map(|(_, c)| (*c).max(0))
                        .sum::<i64>()
                        .to_string(),
                ),
            ];
            if let Some((current, latest)) = app.migration {
                l.push(kv("Schema", &format!("v{current} of v{latest}")));
                if current < latest {
                    l.push(Line::from(""));
                    l.push(Line::from(Span::styled(
                        format!("  {} migration(s) pending — press m", latest - current),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            l
        }
        Tab::Verify => {
            let mut l = vec![Line::from(Span::styled(
                format!(
                    "  {}",
                    crate::tui::app::VERIFY_ENTRIES
                        .get(app.selected_index())
                        .map(|(_, d)| *d)
                        .unwrap_or("")
                ),
                Style::default().fg(MUTED),
            ))];
            l.push(Line::from(""));
            l.push(Line::from(Span::styled(
                "  ⏎ to run",
                Style::default().fg(MUTED),
            )));

            if let Some(outcome) = &app.verify_result {
                l.push(Line::from(""));
                l.push(Line::from(vec![
                    Span::styled("  Last result: ", Style::default().fg(MUTED)),
                    Span::styled(
                        outcome.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        if outcome.ok {
                            "  accepted"
                        } else {
                            "  rejected"
                        },
                        Style::default().fg(if outcome.ok { Color::Green } else { Color::Red }),
                    ),
                ]));
                for (k, v) in &outcome.lines {
                    l.push(kv(k, v));
                }
            }
            l
        }
        Tab::Doctor => match app.doctor.get(app.selected_index()) {
            Some(section) => section
                .checks
                .iter()
                .map(|c| {
                    Line::from(vec![
                        Span::styled(
                            "  ● ",
                            Style::default().fg(if c.ok { Color::Green } else { Color::Red }),
                        ),
                        Span::styled(format!("{:<26}", c.label), Style::default()),
                        Span::styled(c.detail.clone(), Style::default().fg(MUTED)),
                    ])
                })
                .collect(),
            None => vec![Line::from(Span::styled(
                "  running checks…",
                Style::default().fg(MUTED),
            ))],
        },
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(" Detail ");
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn kv<'a>(key: &'a str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {key:>12}  "), Style::default().fg(MUTED)),
        Span::raw(value.to_string()),
    ])
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    // A toast, when present, replaces the key hints — it is the more urgent
    // of the two and the bar is one line.
    let line = match &app.toast {
        Some(toast) => Line::from(Span::styled(
            format!(" {} ", toast.text),
            Style::default()
                .fg(if toast.is_error {
                    Color::Red
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )),
        None => Line::from(Span::styled(
            format!(" {} ", app.hints()),
            Style::default().fg(MUTED),
        )),
    };
    frame.render_widget(Paragraph::new(line), area);
}

// ---- Modals -------------------------------------------------------------

fn draw_confirm(frame: &mut Frame, title: &str, body: &str) {
    let area = centered(frame.area(), 66, 12);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(format!(" {title} "));

    // Indent the body to match the padding the other modals use.
    let indented: String = body
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!("\n{indented}\n\n  y confirm   ·   n cancel");
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_form(frame: &mut Frame, title: &str, fields: &[super::app::Field], active: usize) {
    let height = fields.len() as u16 * 2 + 5;
    let area = centered(frame.area(), 70, height);
    frame.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for (i, field) in fields.iter().enumerate() {
        let is_active = i == active;
        let marker = if is_active { "▌" } else { " " };
        let label_style = if is_active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker} {:<28}", field.label), label_style),
            Span::raw(field.value.clone()),
            Span::styled(
                if is_active { "▌" } else { "" },
                Style::default().fg(ACCENT),
            ),
        ]));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  ⇥ next field   ·   ⏎ submit   ·   esc cancel",
        Style::default().fg(MUTED),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(format!(" {title} "));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_help(frame: &mut Frame, app: &App) {
    let keys = app.help_lines();
    let area = centered(frame.area(), 64, keys.len() as u16 + 4);
    frame.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for (key, description) in keys {
        lines.push(Line::from(vec![
            Span::styled(format!("  {key:>12}  "), Style::default().fg(ACCENT)),
            Span::raw(description),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(format!(" Keys — {} ", app.tab.title()));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_detail(frame: &mut Frame, title: &str, body: &str, scroll: u16) {
    let full = frame.area();
    let area = centered(
        full,
        full.width.saturating_sub(8),
        full.height.saturating_sub(4),
    );
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(format!(" {title} "));

    frame.render_widget(
        Paragraph::new(body.to_string())
            .block(block)
            .scroll((scroll, 0)),
        area,
    );
}

/// A centred rect of the given size, clamped to the available area so a small
/// terminal degrades instead of panicking on an out-of-bounds rect.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_clamps_to_small_terminals() {
        // A modal larger than the terminal must shrink rather than produce a
        // rect that extends past the buffer — ratatui panics on those.
        let area = Rect::new(0, 0, 20, 5);
        let r = centered(area, 70, 12);
        assert_eq!(r.width, 20);
        assert_eq!(r.height, 5);
        assert!(r.x + r.width <= area.width);
        assert!(r.y + r.height <= area.height);
    }

    #[test]
    fn centered_centres_when_it_fits() {
        let area = Rect::new(0, 0, 100, 40);
        let r = centered(area, 60, 10);
        assert_eq!(r.width, 60);
        assert_eq!(r.x, 20);
        assert_eq!(r.y, 15);
    }
}

#[cfg(test)]
pub mod render_tests_support {
    use super::*;
    use crate::context::ProjectContext;
    use crate::tui::app::{App, Modal, Pending, Tab};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    pub fn app() -> App {
        App::for_test(ProjectContext {
            root: Some(PathBuf::from("/tmp/project")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
        })
    }

    pub fn render_to_string(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn sample_frames() -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut a = app();
        out.push(("tab bar at 80 columns", render_to_string(&a, 80, 8)));
        a.tab = Tab::Verify;
        out.push(("verify tab", render_to_string(&a, 100, 16)));
        a.run_doctor();
        a.tab = Tab::Doctor;
        out.push(("doctor tab", render_to_string(&a, 100, 18)));
        a.tab = Tab::Credentials;
        a.tab = Tab::Database;
        a.db_tables = vec![
            ("credentials".into(), 12),
            ("organizations".into(), 3),
            ("role_assessments".into(), 7),
        ];
        out.push(("database tab", render_to_string(&a, 110, 20)));
        a.modal = Modal::Help;
        out.push(("help modal", render_to_string(&a, 110, 24)));
        a.modal = Modal::Confirm {
            title: "Revoke permanently?".into(),
            body: "urn:uuid:abc\n\nRevocation cannot be undone.".into(),
            action: Pending::ExportBundle,
        };
        out.push(("confirm modal", render_to_string(&a, 110, 20)));
        out
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::context::ProjectContext;
    use crate::tui::app::{App, Field, Modal, Pending, Screen, Tab};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::for_test(ProjectContext {
            root: Some(PathBuf::from("/tmp/project")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
        })
    }

    /// Render into an off-screen buffer and return it as text.
    fn render(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn unlock_screen_masks_the_password() {
        let mut app = test_app();
        app.screen = Screen::Unlock {
            password: "hunter2".into(),
            error: None,
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("unlock vault"));
        assert!(out.contains("•••••••"), "password is masked");
        assert!(
            !out.contains("hunter2"),
            "the password never reaches the screen"
        );
    }

    #[test]
    fn unlock_screen_shows_the_failure_reason() {
        let mut app = test_app();
        app.screen = Screen::Unlock {
            password: String::new(),
            error: Some("wrong password".into()),
        };
        assert!(render(&app, 80, 24).contains("wrong password"));
    }

    #[test]
    fn browse_screen_renders_tabs_and_the_empty_state() {
        let app = test_app();
        let out = render(&app, 100, 24);
        assert!(out.contains("Credentials"));
        assert!(out.contains("Orgs"));
        assert!(out.contains("nothing here yet"));
    }

    #[test]
    fn browse_screen_lists_rows_and_detail() {
        let mut app = test_app();
        app.tab = Tab::Database;
        app.db_tables = vec![("credentials".into(), 12), ("organizations".into(), 3)];
        let out = render(&app, 100, 24);
        assert!(out.contains("credentials"));
        assert!(out.contains("12"));
        assert!(out.contains("Tables"), "the detail pane renders alongside");
    }

    #[test]
    fn a_toast_replaces_the_key_hints() {
        let mut app = test_app();
        assert!(render(&app, 100, 24).contains("q quit"));
        app.set_toast_for_test("Revoked urn:uuid:abc", false);
        assert!(render(&app, 100, 24).contains("Revoked urn:uuid:abc"));
    }

    #[test]
    fn modals_render_over_the_browse_screen() {
        let mut app = test_app();
        app.modal = Modal::Confirm {
            title: "Revoke permanently?".into(),
            body: "urn:uuid:abc".into(),
            action: Pending::ExportBundle,
        };
        let out = render(&app, 100, 24);
        assert!(out.contains("Revoke permanently?"));
        assert!(out.contains("confirm"));
    }

    #[test]
    fn rendering_survives_a_tiny_terminal() {
        // ratatui panics on a rect that leaves the buffer, so the modal
        // geometry has to degrade rather than overflow. An 8x4 terminal is
        // far smaller than the 70x12 the form asks for.
        let mut app = test_app();
        app.modal = Modal::Form {
            title: "New organization".into(),
            fields: vec![Field::for_test("Name", true)],
            active: 0,
            action: Pending::CreateOrg,
        };
        render(&app, 8, 4);

        app.screen = Screen::Unlock {
            password: "x".into(),
            error: Some("a very long error message that cannot possibly fit".into()),
        };
        render(&app, 8, 4);
    }
}

#[cfg(test)]
mod visual_dump {
    use super::render_tests_support::*;

    /// Not an assertion — prints the rendered frames so a human can eyeball
    /// the layout. Run with: cargo test visual_dump -- --nocapture --ignored
    #[test]
    #[ignore]
    fn dump_frames() {
        for (name, text) in sample_frames() {
            println!("\n=== {name} ===\n{text}");
        }
    }
}
