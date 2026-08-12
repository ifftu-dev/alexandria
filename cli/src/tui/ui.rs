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

/// Build the wordmark as gradient-coloured lines, sharing the colour ramp with
/// the CLI banner so the two surfaces look like one product.
fn wordmark_lines() -> Vec<Line<'static>> {
    let rows = crate::output::WORDMARK;
    let width = rows[0].chars().count();
    rows.iter()
        .enumerate()
        .map(|(y, row)| {
            let spans: Vec<Span> = row
                .chars()
                .enumerate()
                .map(|(x, ch)| {
                    // Same diagonal sweep as the CLI banner, so the two
                    // surfaces cannot drift apart.
                    let t = crate::output::gradient_t(x, y, width, rows.len());
                    let (r, g, b) = crate::output::gradient_at(t);
                    Span::styled(ch.to_string(), Style::default().fg(Color::Rgb(r, g, b)))
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn draw_unlock(frame: &mut Frame, app: &App) {
    let Screen::Unlock { password, error } = &app.screen else {
        return;
    };

    // The wordmark is 57 columns and 4 rows; drop it rather than let it wrap
    // into noise when the terminal cannot hold it alongside the prompt.
    let word_width = crate::output::WORDMARK[0].chars().count() as u16;
    let full = frame.area();
    let show_wordmark = full.width >= word_width + 6 && full.height >= 16;
    let modal_width = if show_wordmark { word_width + 4 } else { 60 };
    let height = if show_wordmark { 15 } else { 9 };

    let area = centered(full, modal_width, height);
    frame.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();
    if show_wordmark {
        lines.push(Line::from(""));
        for row in wordmark_lines() {
            // One column of padding inside the border.
            let mut spans = vec![Span::raw(" ")];
            spans.extend(row.spans);
            lines.push(Line::from(spans));
        }
    }

    let masked = "\u{2022}".repeat(password.chars().count());
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Password  ", Style::default().fg(MUTED)),
        Span::styled(masked, Style::default().fg(ACCENT)),
        Span::styled("\u{258c}", Style::default().fg(ACCENT)),
    ]));
    lines.push(Line::from(""));

    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  \u{23ce} unlock \u{00b7} esc quit",
        Style::default().fg(MUTED),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" unlock vault ");

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
        Modal::Rows(view) => draw_rows(frame, view),
        Modal::Row {
            rows,
            pairs,
            scroll,
        } => draw_row(frame, &rows.table, rows.selected + 1, pairs, *scroll),
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    // Three forms, widest first. The numbers are the point — they are the
    // shortcut — so counts are dropped before them, and the labels are dropped
    // before the numbers.
    let labelled: Vec<String> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{} {}", i + 1, t.title()))
        .collect();
    let with_counts: Vec<String> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if t.shows_count() {
                format!("{} {} ({})", i + 1, t.title(), tab_count(app, *t))
            } else {
                format!("{} {}", i + 1, t.title())
            }
        })
        .collect();
    let numbers: Vec<String> = (1..=Tab::ALL.len()).map(|i| i.to_string()).collect();

    // Two border columns, plus ratatui's " │ " between each pair.
    let fits = |items: &[String]| -> bool {
        let content: usize = items.iter().map(|i| i.chars().count()).sum();
        content + 3 * items.len().saturating_sub(1) + 2 <= area.width as usize
    };

    let titles = if fits(&with_counts) {
        with_counts
    } else if fits(&labelled) {
        labelled
    } else {
        numbers
    };

    let index = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles.into_iter().map(Line::from).collect::<Vec<_>>())
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

fn tab_count(app: &App, tab: Tab) -> usize {
    match tab {
        Tab::Credentials => app.visible_credentials().len(),
        Tab::Roles => app.assessments.len(),
        Tab::Organizations => app.organizations.len(),
        Tab::Database => app.db_tables.len(),
        _ => 0,
    }
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    // The log stream wants the whole width: a list/detail split would leave
    // messages truncated in a narrow column with nothing in the other one.
    if app.tab == Tab::Logs {
        return draw_logs(frame, app, area);
    }

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
        // Rendered as a stream by draw_logs, not as selectable rows.
        Tab::Logs => Vec::new(),
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

/// Draw the captured log as a stream.
fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let entries = crate::tui::logs::entries(app.log_level);
    let total = crate::tui::logs::len();

    let title = format!(
        " Logs — {} shown of {total} · {} and above{} ",
        entries.len(),
        app.log_level,
        if app.log_follow { " · following" } else { "" }
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(title);

    if entries.is_empty() {
        let hint = if total == 0 {
            "  nothing logged yet"
        } else {
            "  nothing at this level — press l to widen"
        };
        frame.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    let rows = area.height.saturating_sub(2) as usize;
    // Following pins the view to the newest line; otherwise the reader's
    // scroll position stands, clamped so it cannot run off the end.
    let first = if app.log_follow {
        entries.len().saturating_sub(rows)
    } else {
        app.log_offset.min(entries.len().saturating_sub(1))
    };

    let lines: Vec<Line> = entries
        .iter()
        .skip(first)
        .take(rows)
        .map(|e| {
            Line::from(vec![
                Span::styled(format!("{} ", e.time), Style::default().fg(MUTED)),
                Span::styled(
                    format!("{:<5} ", e.level),
                    Style::default()
                        .fg(level_color(e.level))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", e.target), Style::default().fg(ACCENT)),
                Span::raw(e.message.clone()),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn level_color(level: log::Level) -> Color {
    match level {
        log::Level::Error => Color::Red,
        log::Level::Warn => Color::Yellow,
        log::Level::Info => Color::Green,
        log::Level::Debug => Color::Cyan,
        log::Level::Trace => MUTED,
    }
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
            if let Some(help) = crate::tui::app::VERIFY_HELP.get(app.selected_index()) {
                l.push(Line::from(""));
                for chunk in wrap_text(help, 46) {
                    l.push(Line::from(Span::styled(
                        format!("  {chunk}"),
                        Style::default().fg(MUTED),
                    )));
                }
            }
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
        Tab::Logs => Vec::new(),
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
            Span::styled(format!("  {marker} {:<32}", field.label), label_style),
            Span::raw(field_display(&field.value)),
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

/// Draw a page of table rows as a grid.
///
/// Column widths are sized to their contents rather than split evenly: an `id`
/// column and a `json` column in the same table need very different room, and
/// an even split wastes most of the screen on the narrow one.
fn draw_rows(frame: &mut Frame, view: &crate::tui::app::TableRows) {
    let full = frame.area();
    let area = centered(
        full,
        full.width.saturating_sub(4),
        full.height.saturating_sub(2),
    );
    frame.render_widget(Clear, area);

    let shown = view.rows.len();
    let title = if view.total > shown as i64 {
        format!(" {} — {} of {} rows ", view.table, shown, view.total)
    } else {
        format!(" {} — {} rows ", view.table, shown)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title);

    if view.columns.is_empty() || view.rows.is_empty() {
        frame.render_widget(
            Paragraph::new("  this table is empty")
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    // Inside the border, minus the row-number gutter.
    let inner_width = area.width.saturating_sub(2) as usize;
    let gutter = 6usize;
    let avail = inner_width.saturating_sub(gutter);

    // Width each visible column wants, capped so one wide column cannot push
    // every other column off the screen.
    let widths: Vec<usize> = view
        .columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let longest = view
                .rows
                .iter()
                .filter_map(|r| r.get(i))
                .map(|v| v.chars().count())
                .max()
                .unwrap_or(0);
            longest.max(name.chars().count()).clamp(3, 32)
        })
        .collect();

    // Take columns from the scroll offset until the row is full.
    let mut visible: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for (i, width) in widths.iter().enumerate().skip(view.col_offset) {
        let w = width + 1;
        if used + w > avail && !visible.is_empty() {
            break;
        }
        used += w;
        visible.push(i);
    }

    let cell = |text: &str, w: usize| -> String {
        let count = text.chars().count();
        if count > w {
            let kept: String = text.chars().take(w.saturating_sub(1)).collect();
            format!("{kept}…")
        } else {
            format!("{text:<w$}")
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    let mut header = vec![Span::styled(
        format!("{:<gutter$}", "#"),
        Style::default().fg(MUTED),
    )];
    for &i in &visible {
        header.push(Span::styled(
            format!("{} ", cell(&view.columns[i], widths[i])),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(header));

    // The window follows the selection rather than being tracked separately,
    // so the highlighted row is always on screen by construction.
    let body_rows = area.height.saturating_sub(3) as usize;
    let first = if body_rows > 0 && view.selected >= body_rows {
        view.selected + 1 - body_rows
    } else {
        0
    };

    for (n, row) in view.rows.iter().enumerate().skip(first).take(body_rows) {
        let current = n == view.selected;
        let base = if current {
            Style::default().bg(ACCENT).fg(Color::Black)
        } else {
            Style::default()
        };
        let mut spans = vec![Span::styled(
            format!("{:<gutter$}", n + 1),
            if current {
                base.add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        )];
        for &i in &visible {
            let value = row.get(i).map(String::as_str).unwrap_or("");
            let style = if current {
                base
            } else if value == "NULL" {
                Style::default().fg(MUTED)
            } else {
                Style::default()
            };
            spans.push(Span::styled(format!("{} ", cell(value, widths[i])), style));
        }
        lines.push(Line::from(spans));
    }

    // Say what is off-screen; silently truncating reads as "that is all there
    // is".
    let hidden_left = view.col_offset;
    let hidden_right = view.columns.len() - visible.last().map(|i| i + 1).unwrap_or(0);
    if hidden_left > 0 || hidden_right > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ← {hidden_left} more   {hidden_right} more →"),
            Style::default().fg(MUTED),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Draw one row: every column, values not clipped to a grid cell.
///
/// Laid out as label-above-value rather than in two columns, because the
/// values this view exists for — JSON payloads, DIDs, signatures — are far
/// wider than any label column would leave room for.
fn draw_row(
    frame: &mut Frame,
    table: &str,
    number: usize,
    pairs: &[(String, String)],
    scroll: u16,
) {
    let full = frame.area();
    let area = centered(
        full,
        full.width.saturating_sub(4),
        full.height.saturating_sub(2),
    );
    frame.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();
    for (name, value) in pairs {
        lines.push(Line::from(Span::styled(
            format!("  {name}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        let style = if value == "NULL" {
            Style::default().fg(MUTED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(format!("    {value}"), style)));
        lines.push(Line::from(""));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(format!(" {table} — row {number} "));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            // Wrapped, so a long value is readable rather than running off the
            // right edge where the grid already showed it truncated.
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

/// Wrap on word boundaries. `Paragraph`'s own wrapping applies to the whole
/// block; these lines need to wrap at the detail pane's width while sitting
/// among lines that must not.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// What to show in a form field.
///
/// A pasted credential is thousands of characters across many lines; printing
/// it into a single-line field would flood the modal and tell the reader
/// nothing. Summarise anything that large, and flatten newlines out of what is
/// left so the layout survives.
fn field_display(value: &str) -> String {
    const VISIBLE: usize = 28;
    let count = value.chars().count();
    if count > VISIBLE {
        let trimmed = value.trim_start();
        let kind = if trimmed.starts_with('{') || trimmed.starts_with('[') {
            "JSON"
        } else {
            "text"
        };
        return format!("<{kind}, {count} chars>");
    }
    value.replace(['\n', '\r', '\t'], " ")
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
    use crate::context::{ProfileSelection, ProjectContext};
    use crate::tui::app::{App, Field, Modal, Pending, Tab};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    pub fn app() -> App {
        App::for_test(ProjectContext {
            root: Some(PathBuf::from("/tmp/project")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
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

    /// A fixture standing in for a real query result, so the grid can be
    /// exercised without a decrypted database.
    pub fn sample_rows() -> crate::tui::app::TableRows {
        crate::tui::app::TableRows {
            table: "credentials".into(),
            columns: vec![
                "id".into(),
                "subject".into(),
                "type".into(),
                "issued_at".into(),
                "payload".into(),
            ],
            rows: vec![
                vec![
                    "urn:uuid:aaa".into(),
                    "did:key:z6MkAlice".into(),
                    "SkillCredential".into(),
                    "2026-01-01T00:00:00Z".into(),
                    "<blob 2048 bytes>".into(),
                ],
                vec![
                    "urn:uuid:bbb".into(),
                    "did:key:z6MkBob".into(),
                    "RoleCredential".into(),
                    "2026-02-14T09:30:00Z".into(),
                    "NULL".into(),
                ],
                vec![
                    "urn:uuid:ccc".into(),
                    "did:key:z6MkCarol".into(),
                    "SkillCredential".into(),
                    "2026-03-02T11:00:00Z".into(),
                    "eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ1cm46dXVpZDpjY2MifQ".into(),
                ],
            ],
            total: 128,
            selected: 0,
            col_offset: 0,
        }
    }

    pub fn sample_frames() -> Vec<(&'static str, String)> {
        use crate::tui::app::Screen;
        let mut out = Vec::new();
        let mut a = app();
        a.screen = Screen::Unlock {
            password: "hunter2".into(),
            error: None,
        };
        out.push(("unlock screen", render_to_string(&a, 80, 24)));
        a.screen = Screen::Unlock {
            password: String::new(),
            error: Some("Incorrect vault password".into()),
        };
        out.push(("unlock, wrong password", render_to_string(&a, 80, 20)));
        a.screen = Screen::Unlock {
            password: "abc".into(),
            error: None,
        };
        out.push(("unlock in a small terminal", render_to_string(&a, 44, 12)));
        a.screen = Screen::Browse;
        a.credentials = vec![];
        a.db_tables = (0..34).map(|i| (format!("t{i}"), 0)).collect();
        out.push(("tab bar at 120 columns", render_to_string(&a, 120, 6)));
        out.push(("tab bar at 80 columns", render_to_string(&a, 80, 6)));
        out.push(("tab bar at 40 columns", render_to_string(&a, 40, 6)));
        a.db_tables = Vec::new();
        crate::tui::logs::clear();
        crate::tui::logs::install();
        log::info!("alexandria tui 0.1.0 starting");
        log::info!("vault unlocked for profile HackRidesTest (52fce5a0)");
        log::debug!("refreshed: 6 credentials, 2 assessments, 1 organizations, 34 tables");
        log::warn!("status list for urn:uuid:aaa not found — treating as not revoked");
        log::error!("action failed: Incorrect vault password");
        a.tab = Tab::Logs;
        out.push(("logs tab", render_to_string(&a, 108, 12)));
        a.log_level = log::LevelFilter::Warn;
        out.push(("logs, filtered to warn", render_to_string(&a, 108, 9)));
        a.log_level = log::LevelFilter::Debug;
        a.tab = Tab::Verify;
        out.push(("verify tab", render_to_string(&a, 100, 18)));
        a.modal = Modal::Form {
            title: "Verify credential bundle".into(),
            fields: vec![
                {
                    let mut f = Field::for_test("Bundle (path or pasted JSON)", true);
                    f.value = format!(
                        "{{\"formatVersion\":1,\"credentials\":[{}]}}",
                        "x".repeat(900)
                    );
                    f
                },
                Field::for_test("Verify as of (default: now)", false),
            ],
            active: 0,
            action: Pending::VerifyBundle,
        };
        out.push((
            "verify form with a pasted bundle",
            render_to_string(&a, 100, 14),
        ));
        a.modal = Modal::None;
        a.run_doctor();
        a.tab = Tab::Doctor;
        out.push(("doctor tab", render_to_string(&a, 100, 18)));
        a.tab = Tab::Database;
        a.modal = Modal::Rows(sample_rows());
        out.push(("table rows", render_to_string(&a, 100, 14)));
        let mut scrolled = sample_rows();
        scrolled.col_offset = 3;
        scrolled.selected = 1;
        a.modal = Modal::Rows(scrolled);
        out.push(("table rows, scrolled right", render_to_string(&a, 60, 12)));
        let grid = sample_rows();
        let pairs: Vec<(String, String)> = grid
            .columns
            .iter()
            .cloned()
            .zip(grid.rows[2].iter().cloned())
            .collect();
        a.modal = Modal::Row {
            rows: grid,
            pairs,
            scroll: 0,
        };
        out.push(("expanded row", render_to_string(&a, 76, 18)));
        a.modal = Modal::None;
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
    use crate::context::{ProfileSelection, ProjectContext};
    use crate::tui::app::{App, Field, Modal, Pending, Screen, Tab};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::for_test(ProjectContext {
            root: Some(PathBuf::from("/tmp/project")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
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
    fn unlock_screen_shows_the_wordmark_and_drops_it_when_cramped() {
        let mut app = test_app();
        app.screen = Screen::Unlock {
            password: String::new(),
            error: None,
        };
        // The wordmark's first row is distinctive enough to assert on.
        let first_row = crate::output::WORDMARK[0];
        let roomy = render(&app, 80, 24);
        assert!(roomy.contains(first_row), "wordmark missing at 80x24");
        assert!(roomy.contains("Password"));

        // Too narrow or too short: the prompt survives, the wordmark does not
        // — a wrapped wordmark is unreadable noise.
        let cramped = render(&app, 44, 12);
        assert!(!cramped.contains(first_row), "wordmark should be dropped");
        assert!(cramped.contains("Password"), "the prompt must still render");
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
    fn the_tab_bar_shows_shortcut_numbers_and_never_wraps() {
        let mut app = test_app();
        app.db_tables = (0..34).map(|i| (format!("t{i}"), 0)).collect();

        // Wide: numbers, labels and counts all fit.
        let wide = render(&app, 120, 8);
        assert!(wide.contains("1 Credentials"), "missing numbers: {wide}");
        assert!(wide.contains("4 Database (34)"), "missing counts: {wide}");
        assert!(wide.contains("7 Logs"));

        // Narrow: counts are dropped before the numbers, because the numbers
        // are the shortcut.
        let narrow = render(&app, 80, 8);
        assert!(
            narrow.contains("1 Credentials"),
            "numbers must survive: {narrow}"
        );
        assert!(
            !narrow.contains("(34)"),
            "counts should have been dropped: {narrow}"
        );

        // Every rendered line must fit the terminal, or the bar has wrapped.
        for line in narrow.lines() {
            assert!(line.chars().count() <= 80, "line overflows: {line}");
        }

        // Very narrow: numbers alone, still usable as shortcuts.
        let tiny = render(&app, 40, 8);
        assert!(tiny.contains("1 │ 2"), "expected bare numbers: {tiny}");
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
    fn a_large_paste_is_summarised_rather_than_dumped() {
        // A pasted credential is thousands of characters; printing it into a
        // single-line field would flood the modal.
        let big = format!("{{\"vc\":\"{}\"}}", "x".repeat(4000));
        let shown = field_display(&big);
        assert!(shown.starts_with("<JSON, "), "got: {shown}");
        assert!(shown.contains(&big.chars().count().to_string()));
        assert!(shown.chars().count() < 40);

        // Short values are shown as typed, with newlines flattened so a
        // stray one cannot break the row.
        assert_eq!(field_display("acme"), "acme");
        assert_eq!(field_display("a\nb"), "a b");
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
