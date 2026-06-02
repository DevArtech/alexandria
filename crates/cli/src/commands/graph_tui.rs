//! Interactive terminal graph explorer (local library only).

use std::io;
use std::path::PathBuf;

use alexandria_core::{
    build_graph_view, Config, GraphScope, GraphView, GraphViewOptions, Index, Library, Rel,
};
use anyhow::{bail, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;
use std::io::Stdout;

use super::graph::GraphScopeArg;
use crate::commands::util::parse_rel_cli;

struct AppState {
    view: GraphView,
    selected: usize,
    focus_id: Option<String>,
    detail: Option<String>,
    status: String,
}

pub fn run(
    library_path: Option<PathBuf>,
    seed: Option<String>,
    scope: GraphScopeArg,
    depth: Option<u32>,
    rels: Vec<String>,
    max_nodes: usize,
    overlay_facets: bool,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;

    if matches!(scope, GraphScopeArg::Seed | GraphScopeArg::Provenance) && seed.is_none() {
        bail!("--seed is required for seed and provenance scopes in interactive mode");
    }

    let parsed_rels = if rels.is_empty() {
        None
    } else {
        Some(
            rels.iter()
                .map(|r| parse_rel_cli(r))
                .collect::<Result<Vec<Rel>, _>>()?,
        )
    };

    let view = build_graph_view(
        &index,
        &config,
        GraphViewOptions {
            scope: scope.into(),
            seed: seed.clone(),
            depth: depth.unwrap_or(2),
            rels: parsed_rels.clone(),
            max_nodes,
            overlay_facets,
        },
    )?;

    let focus_id = seed.or_else(|| view.nodes.first().map(|n| n.id.clone()));

    let mut app = AppState {
        view,
        selected: 0,
        focus_id: focus_id.clone(),
        detail: focus_id.and_then(|id| load_detail(&index, &id)),
        status: "j/k: select  Enter: focus+expand  q: quit".to_string(),
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(
        &mut terminal,
        &mut app,
        &index,
        &config,
        parsed_rels,
        max_nodes,
        overlay_facets,
    );

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend>,
    app: &mut AppState,
    index: &Index,
    config: &Config,
    rels: Option<Vec<Rel>>,
    max_nodes: usize,
    overlay_facets: bool,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        if !app.view.nodes.is_empty() {
                            app.selected = (app.selected + 1).min(app.view.nodes.len() - 1);
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.selected = app.selected.saturating_sub(1);
                    }
                    KeyCode::Enter => {
                        if let Some(node) = app.view.nodes.get(app.selected) {
                            let id = node.id.clone();
                            app.focus_id = Some(id.clone());
                            app.detail = load_detail(index, &id);
                            if let Ok(expanded) = expand_from_focus(
                                index,
                                config,
                                &id,
                                rels.clone(),
                                max_nodes,
                                overlay_facets,
                            ) {
                                app.view = expanded;
                                app.selected =
                                    app.view.nodes.iter().position(|n| n.id == id).unwrap_or(0);
                                app.status = format!("expanded from {id}");
                            }
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

type CrosstermBackend = ratatui::backend::CrosstermBackend<Stdout>;

fn expand_from_focus(
    index: &Index,
    config: &Config,
    focus_id: &str,
    rels: Option<Vec<Rel>>,
    max_nodes: usize,
    overlay_facets: bool,
) -> Result<GraphView> {
    build_graph_view(
        index,
        config,
        GraphViewOptions {
            scope: GraphScope::Seed,
            seed: Some(focus_id.to_string()),
            depth: 2,
            rels,
            max_nodes,
            overlay_facets,
        },
    )
    .map_err(Into::into)
}

fn tier_label(tier: alexandria_core::Tier) -> &'static str {
    match tier {
        alexandria_core::Tier::Working => "working",
        alexandria_core::Tier::Episodic => "episodic",
        alexandria_core::Tier::Provisional => "provisional",
        alexandria_core::Tier::Semantic => "semantic",
        alexandria_core::Tier::Procedural => "procedural",
        alexandria_core::Tier::Relational => "relational",
    }
}

fn load_detail(index: &Index, id: &str) -> Option<String> {
    index.get_engram(id).ok().flatten().map(|row| {
        format!(
            "id: {}\ntier: {}\nstatus: {}\nconfidence: {:.2}\nclaim: {}\n\ncollections: {}\nlinks: {}",
            row.id,
            tier_label(row.tier),
            status_label(row.status),
            row.confidence,
            row.claim,
            if row.collections.is_empty() {
                "(none)".into()
            } else {
                row.collections.join(", ")
            },
            row.links.len(),
        )
    })
}

fn status_label(status: alexandria_core::Status) -> &'static str {
    match status {
        alexandria_core::Status::Confirmed => "confirmed",
        alexandria_core::Status::Provisional => "provisional",
        alexandria_core::Status::UnresolvedByDesign => "unresolved_by_design",
        alexandria_core::Status::Superseded => "superseded",
        alexandria_core::Status::Archived => "archived",
    }
}

fn ui(f: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    let header = Paragraph::new(format!(
        "Alexandria graph explorer | scope: {} | nodes: {} | edges: {}{}",
        app.view.scope,
        app.view.node_count,
        app.view.edge_count,
        if app.view.truncated {
            " (truncated)"
        } else {
            ""
        }
    ))
    .block(Block::default().borders(Borders::ALL).title("graph"));
    f.render_widget(header, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    render_nodes(f, app, body[0]);
    render_edges_and_detail(f, app, body[1]);

    let footer = Paragraph::new(app.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("keys"));
    f.render_widget(footer, chunks[2]);
}

fn render_nodes(f: &mut Frame, app: &AppState, area: Rect) {
    let items: Vec<ListItem> = app
        .view
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let marker = if app.focus_id.as_deref() == Some(node.id.as_str()) {
                "* "
            } else {
                "  "
            };
            let style = node_style(node, i == app.selected);
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(format!("[{}] ", node.id), Style::default().fg(Color::Cyan)),
                Span::styled(truncate(&node.claim, 36), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("nodes"));
    f.render_widget(list, area);
}

fn render_edges_and_detail(f: &mut Frame, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let selected_id = app
        .view
        .nodes
        .get(app.selected)
        .map(|n| n.id.as_str())
        .unwrap_or("");

    let edge_items: Vec<ListItem> = app
        .view
        .edges
        .iter()
        .filter(|e| e.from_id == selected_id || e.to_id == selected_id)
        .map(|e| ListItem::new(format!("{} --{}--> {}", e.from_id, e.rel, e.to_id)))
        .collect();

    let edges = List::new(if edge_items.is_empty() {
        vec![ListItem::new("(no edges for selection)")]
    } else {
        edge_items
    })
    .block(Block::default().borders(Borders::ALL).title("edges"));
    f.render_widget(edges, chunks[0]);

    let detail_text = app
        .detail
        .clone()
        .unwrap_or_else(|| "Select a node and press Enter to focus.".to_string());
    let detail = Paragraph::new(detail_text)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("detail"));
    f.render_widget(detail, chunks[1]);
}

fn node_style(node: &alexandria_core::GraphViewNode, selected: bool) -> Style {
    let mut style = Style::default();
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    match node.status.as_str() {
        "superseded" | "archived" => style = style.fg(Color::DarkGray),
        "unresolved_by_design" => style = style.add_modifier(Modifier::BOLD),
        _ => {}
    }
    match node.tier.as_str() {
        "semantic" => style = style.fg(if selected { Color::White } else { Color::Green }),
        "episodic" => style = style.fg(if selected { Color::White } else { Color::Blue }),
        "provisional" => {
            style = style.fg(if selected {
                Color::White
            } else {
                Color::Yellow
            })
        }
        _ => {}
    }
    style
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
