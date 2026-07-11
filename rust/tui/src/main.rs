use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ed25519_dalek::SigningKey;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use comms_core::archive::{self, ManifestLevel};
use comms_core::config;
use comms_core::now_rfc3339;
use comms_core::signing::{self, AppraisalState, PendingView};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Pending,
    Manifest,
}

enum Mode {
    Normal,
    Clarify,
}

struct Args {
    root: PathBuf,
    archive: Option<PathBuf>,
    key: Option<PathBuf>,
    check: bool,
}

struct App {
    root: PathBuf,
    archive: PathBuf,
    key: Option<SigningKey>,
    tab: Tab,
    mode: Mode,
    pending: Vec<PendingView>,
    selected: usize,
    manifest_level: ManifestLevel,
    manifest: String,
    manifest_scroll: u16,
    input: String,
    status: String,
    quit: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let key = match &args.key {
        Some(path) => Some(signing::load_signing_key(path).map_err(io::Error::other)?),
        None => None,
    };
    let archive = args
        .archive
        .unwrap_or_else(|| configured_archive(&args.root));
    let mut app = App {
        root: args.root,
        archive,
        key,
        tab: Tab::Pending,
        mode: Mode::Normal,
        pending: Vec::new(),
        selected: 0,
        manifest_level: ManifestLevel::Minimal,
        manifest: String::new(),
        manifest_scroll: 0,
        input: String::new(),
        status: String::new(),
        quit: false,
    };
    app.refresh();
    if args.check {
        println!(
            "pending={} archive={} manifest=minimal",
            app.pending.len(),
            app.archive.display()
        );
        if !app.status.is_empty() {
            println!("status={}", app.status);
        }
        return Ok(());
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result.map_err(Into::into)
}

fn parse_args() -> Result<Args, String> {
    let mut root = PathBuf::from(".");
    let mut archive = None;
    let mut key = None;
    let mut check = false;
    let mut positionals = Vec::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--archive" | "--key" => {
                let flag = args[i].clone();
                i += 1;
                let value = args.get(i).ok_or_else(|| format!("{flag} needs a path"))?;
                if flag == "--archive" {
                    archive = Some(PathBuf::from(value));
                } else {
                    key = Some(PathBuf::from(value));
                }
            }
            "--check" => check = true,
            "-h" | "--help" => {
                println!("comms-tui [repo-root] [--archive PATH] [--key PATH] [--check]");
                std::process::exit(0);
            }
            value if value.starts_with('-') => return Err(format!("unknown option {value}")),
            value => positionals.push(value.to_owned()),
        }
        i += 1;
    }
    if let Some(value) = positionals.first() {
        root = PathBuf::from(value);
    }
    Ok(Args {
        root,
        archive,
        key,
        check,
    })
}

fn configured_archive(root: &Path) -> PathBuf {
    let comms = root.join(".comms");
    if let Ok(cfg) = config::load(&comms) {
        if let Some(path) = cfg.archive_path {
            let path = PathBuf::from(path);
            return if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
        }
    }
    root.to_path_buf()
}

impl App {
    fn pending_dirs(&self) -> Vec<PathBuf> {
        vec![self.root.join(".comms/pending")]
    }

    fn refresh(&mut self) {
        match signing::discover_pending(&self.pending_dirs()) {
            Ok(pending) => {
                self.pending = pending;
                if self.selected >= self.pending.len() {
                    self.selected = self.pending.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("pending discovery: {e}"),
        }
        match archive::manifest(&self.archive, self.manifest_level) {
            Ok(value) => {
                self.manifest = serde_json::to_string_pretty(&value).unwrap_or_default();
            }
            Err(e) => {
                self.manifest = String::new();
                self.status = format!("manifest: {e}");
            }
        }
    }

    fn export_manifest(&mut self) {
        if self.manifest.is_empty() {
            self.status = "no manifest to export".into();
            return;
        }
        let path = format!("manifest.{}.json", self.manifest_level.name());
        match std::fs::write(&path, &self.manifest) {
            Ok(()) => self.status = format!("exported {} manifest -> {path}", self.manifest_level.name()),
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }

    fn selected(&self) -> Option<&PendingView> {
        self.pending.get(self.selected)
    }

    fn selected_identity(&self) -> Option<(PathBuf, PathBuf, String)> {
        self.selected()
            .map(|v| (v.source.clone(), v.intended_store.clone(), v.stem.clone()))
    }

    fn set_state(&mut self, state: AppraisalState) {
        let Some((source, _, stem)) = self.selected_identity() else {
            self.status = "no pending item selected".into();
            return;
        };
        match signing::set_appraisal_state(&source, &stem, state, None) {
            Ok(path) => self.status = format!("appraisal updated: {}", path.display()),
            Err(e) => self.status = e,
        }
        self.refresh();
    }

    fn sign_selected(&mut self) {
        let Some((source, _, stem)) = self.selected_identity() else {
            self.status = "no pending item selected".into();
            return;
        };
        let Some(key) = &self.key else {
            self.status = "no signer loaded; relaunch with --key PATH".into();
            return;
        };
        match signing::sign_pending_selected(&source, key, std::slice::from_ref(&stem)) {
            Ok(out) => {
                let roles = out
                    .first()
                    .map(|o| o.signed_roles.join(", "))
                    .unwrap_or_default();
                self.status = if roles.is_empty() {
                    "selected key satisfies no outstanding role".into()
                } else {
                    format!("signed {stem} as {roles}")
                };
            }
            Err(e) => self.status = e,
        }
        self.refresh();
    }

    fn finalize_selected(&mut self) {
        let Some((source, store, stem)) = self.selected_identity() else {
            self.status = "no pending item selected".into();
            return;
        };
        match signing::finalize_pending_selected(&source, &store, std::slice::from_ref(&stem)) {
            Ok(_) => self.status = format!("finalized {stem} -> {}", store.display()),
            Err(e) => self.status = e,
        }
        self.refresh();
    }

    fn submit_clarification(&mut self) {
        let Some((source, store, stem)) = self.selected_identity() else {
            self.status = "no pending item selected".into();
            return;
        };
        let Some(key) = &self.key else {
            self.status = "no signer loaded; relaunch with --key PATH".into();
            return;
        };
        if self.input.trim().is_empty() {
            self.status = "clarification question is empty".into();
            return;
        }
        match signing::request_clarification(
            &source,
            &stem,
            self.input.as_bytes(),
            key,
            &store,
            &now_rfc3339(),
        ) {
            Ok(out) => self.status = format!("clarification requested: {}", out.clarification_id),
            Err(e) => self.status = e,
        }
        self.input.clear();
        self.mode = Mode::Normal;
        self.refresh();
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| render(frame, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match app.mode {
                Mode::Clarify => match key.code {
                    KeyCode::Esc => {
                        app.mode = Mode::Normal;
                        app.input.clear();
                        app.status = "clarification cancelled".into();
                    }
                    KeyCode::Enter => app.submit_clarification(),
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Char(c) => app.input.push(c),
                    _ => {}
                },
                Mode::Normal => match key.code {
                    KeyCode::Char('q') => app.quit = true,
                    KeyCode::Tab => {
                        app.tab = if app.tab == Tab::Pending {
                            Tab::Manifest
                        } else {
                            Tab::Pending
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.tab == Tab::Pending {
                            app.selected = app.selected.saturating_sub(1);
                        } else {
                            app.manifest_scroll = app.manifest_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.tab == Tab::Pending && app.selected + 1 < app.pending.len() {
                            app.selected += 1;
                        } else if app.tab == Tab::Manifest {
                            app.manifest_scroll = app.manifest_scroll.saturating_add(1);
                        }
                    }
                    KeyCode::Char('r') => {
                        app.refresh();
                        app.status = "refreshed".into();
                    }
                    KeyCode::Char('e') if app.tab == Tab::Manifest => {
                        app.export_manifest();
                    }
                    KeyCode::Char('m') if app.tab == Tab::Manifest => {
                        app.manifest_level = if app.manifest_level == ManifestLevel::Minimal {
                            ManifestLevel::Full
                        } else {
                            ManifestLevel::Minimal
                        };
                        app.manifest_scroll = 0;
                        app.refresh();
                        app.status = format!("manifest level: {:?}", app.manifest_level);
                    }
                    KeyCode::Char('a') if app.tab == Tab::Pending => {
                        app.set_state(AppraisalState::Approved)
                    }
                    KeyCode::Char('d') if app.tab == Tab::Pending => {
                        app.set_state(AppraisalState::Deferred)
                    }
                    KeyCode::Char('x') if app.tab == Tab::Pending => {
                        app.set_state(AppraisalState::Declined)
                    }
                    KeyCode::Char('Q') if app.tab == Tab::Pending => {
                        app.set_state(AppraisalState::Quarantined)
                    }
                    KeyCode::Char('c') if app.tab == Tab::Pending => {
                        if app.selected().is_some() {
                            app.mode = Mode::Clarify;
                            app.input.clear();
                        }
                    }
                    KeyCode::Char('s') if app.tab == Tab::Pending => app.sign_selected(),
                    KeyCode::Char('f') if app.tab == Tab::Pending => app.finalize_selected(),
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, app: &App) {
    let [tabs, body, help, status] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    let titles = ["Pending desk", "Archive manifest"];
    let selected = if app.tab == Tab::Pending { 0 } else { 1 };
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .block(Block::default().borders(Borders::ALL).title(" comms-tui "))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        tabs,
    );
    match app.tab {
        Tab::Pending => render_pending(frame, app, body),
        Tab::Manifest => render_manifest(frame, app, body),
    }
    let help_text = if app.tab == Tab::Pending {
        "Tab manifest | j/k move | a approve d defer x decline Q quarantine | c clarify | s sign | f finalize | r refresh | q quit"
    } else {
        "Tab pending | j/k scroll | m minimal/full (deliberate disclosure) | e export to file | r refresh | q quit"
    };
    frame.render_widget(
        Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray)),
        help,
    );
    frame.render_widget(
        Paragraph::new(app.status.as_str())
            .block(Block::default().borders(Borders::TOP).title(" status "))
            .style(Style::default().fg(Color::Yellow)),
        status,
    );
    if matches!(app.mode, Mode::Clarify) {
        render_input(frame, app);
    }
}

fn render_pending(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(area);
    let items: Vec<ListItem> = app
        .pending
        .iter()
        .map(|p| {
            let style = match p.appraisal {
                AppraisalState::Approved | AppraisalState::Signed => {
                    Style::default().fg(Color::Green)
                }
                AppraisalState::AwaitingClarification | AppraisalState::Deferred => {
                    Style::default().fg(Color::Yellow)
                }
                AppraisalState::Declined | AppraisalState::Quarantined => {
                    Style::default().fg(Color::Red)
                }
                _ => Style::default(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", p.appraisal.name()), style),
                Span::raw(&p.stem),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(if app.pending.is_empty() {
        None
    } else {
        Some(app.selected)
    });
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" inboxes ({}) ", app.pending.len())),
            )
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().bg(Color::DarkGray)),
        left,
        &mut state,
    );

    let detail = app.selected().map(|p| {
        let needs = p.needs.iter().map(|n| format!("{} as {}", n.by, n.role)).collect::<Vec<_>>().join("\n");
        format!(
            "{}\n\nclaim: {} / {}\nabout: {}\n\ninbox: {}\ndestination: {}\nexisting signatures: {} ({})\nneeds:\n{}\n\nbody ({} bytes):\n{}",
            p.id,
            p.claim_type.as_deref().unwrap_or("unknown"),
            p.kind.as_deref().unwrap_or("unknown"),
            p.about.as_deref().unwrap_or("unknown"),
            p.source.display(),
            p.intended_store.display(),
            p.signatures,
            if p.signatures_valid { "valid" } else { "INVALID" },
            needs,
            p.body_len.unwrap_or(0),
            p.body_text.as_deref().unwrap_or("[detached or binary]"),
        )
    }).unwrap_or_else(|| "No pending items. Discovery checked .comms/pending.".into());
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" exact proposed act "),
        ),
        right,
    );
}

fn render_manifest(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(app.manifest.as_str())
            .scroll((app.manifest_scroll, 0))
            .block(Block::default().borders(Borders::ALL).title(format!(
                " {:?} — {} ",
                app.manifest_level,
                app.archive.display()
            )))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_input(frame: &mut Frame, app: &App) {
    let area = centered(76, 7, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(app.input.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" signed clarification request — Enter submit, Esc cancel "),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered(width_percent: u16, height: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    let [horizontal] = Layout::horizontal([Constraint::Percentage(width_percent)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);
    horizontal
}
