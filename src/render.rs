use crate::error::{Error, Result};
use crate::state::{ChangeKind, DiffUpdate, HunkLine, State};
use crossbeam_channel::{Receiver, RecvTimeoutError, select};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const TICK: Duration = Duration::from_millis(250);

pub fn run(repo_name: &str, updates: Receiver<DiffUpdate>) -> Result<()> {
    let _guard = TerminalGuard::install()?;
    let mut terminal = make_terminal()?;
    let mut state = State::new();
    let mut scroll: u16 = 0;
    let mut sidebar_visible: bool = true;

    let (input_tx, input_rx) = crossbeam_channel::bounded::<InputEvent>(32);
    let stop = Arc::new(AtomicBool::new(false));
    spawn_input_thread(input_tx, Arc::clone(&stop));

    loop {
        let size = terminal.size().map_err(|e| Error::Term { source: e })?;
        let diff_w = diff_inner_width(size.width, sidebar_visible);
        let viewport_h = size.height.saturating_sub(2); // borders + footer
        let max_scroll = content_total_rows(&state, diff_w).saturating_sub(viewport_h);
        scroll = scroll.min(max_scroll);

        terminal
            .draw(|f| draw(f, &state, scroll, sidebar_visible, repo_name))
            .map_err(|e| Error::Term { source: e })?;

        let mut redraw = false;
        select! {
            recv(updates) -> msg => match msg {
                Ok(update) => {
                    state.apply(update);
                    redraw = true;
                }
                Err(_) => break,
            },
            recv(input_rx) -> msg => match msg {
                Ok(InputEvent::Quit) => break,
                Ok(InputEvent::ScrollUp(n)) => {
                    scroll = scroll.saturating_sub(n);
                    redraw = true;
                }
                Ok(InputEvent::ScrollDown(n)) => {
                    scroll = scroll.saturating_add(n);
                    redraw = true;
                }
                Ok(InputEvent::Top) => {
                    scroll = 0;
                    redraw = true;
                }
                Ok(InputEvent::NextFile) => {
                    let area = terminal.size().map(|s| s.width).unwrap_or(80);
                    let offsets = file_offsets(&state, diff_inner_width(area, sidebar_visible));
                    if let Some(&next) = offsets.iter().find(|&&o| o > scroll) {
                        scroll = next;
                    }
                    redraw = true;
                }
                Ok(InputEvent::PrevFile) => {
                    let area = terminal.size().map(|s| s.width).unwrap_or(80);
                    let offsets = file_offsets(&state, diff_inner_width(area, sidebar_visible));
                    if let Some(&prev) = offsets.iter().rev().find(|&&o| o < scroll) {
                        scroll = prev;
                    }
                    redraw = true;
                }
                Ok(InputEvent::ToggleSidebar) => {
                    sidebar_visible = !sidebar_visible;
                    redraw = true;
                }
                Err(_) => break,
            },
            default(TICK) => {}
        }

        if !redraw {
            // tick fall-through; loop will redraw timestamps once we have them
        }
    }

    stop.store(true, Ordering::Relaxed);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum InputEvent {
    Quit,
    ScrollUp(u16),
    ScrollDown(u16),
    Top,
    NextFile,
    PrevFile,
    ToggleSidebar,
}

fn spawn_input_thread(tx: crossbeam_channel::Sender<InputEvent>, stop: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("input".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(err) => {
                        tracing::error!(error = %err, "crossterm poll failed");
                        return;
                    }
                }
                let evt = match event::read() {
                    Ok(e) => e,
                    Err(err) => {
                        tracing::error!(error = %err, "crossterm read failed");
                        return;
                    }
                };
                let Some(out) = translate(evt) else { continue };
                if tx.send(out).is_err() {
                    return;
                }
            }
        })
        .expect("input thread spawn");
}

fn translate(evt: Event) -> Option<InputEvent> {
    let Event::Key(key) = evt else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Some(InputEvent::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(InputEvent::Quit),
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => Some(InputEvent::ScrollDown(1)),
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => Some(InputEvent::ScrollUp(1)),
        (KeyCode::PageDown, _) => Some(InputEvent::ScrollDown(20)),
        (KeyCode::PageUp, _) => Some(InputEvent::ScrollUp(20)),
        (KeyCode::Home, _) | (KeyCode::Char('g'), _) => Some(InputEvent::Top),
        (KeyCode::Char('n'), _) => Some(InputEvent::NextFile),
        (KeyCode::Char('b'), _) => Some(InputEvent::PrevFile),
        (KeyCode::Char('s'), _) => Some(InputEvent::ToggleSidebar),
        _ => None,
    }
}

fn file_offsets(state: &State, diff_width: u16) -> Vec<u16> {
    let mut offsets = Vec::with_capacity(state.len());
    let mut cur: u32 = 0;
    for (i, update) in state.iter_ordered().enumerate() {
        if i > 0 {
            cur = cur.saturating_add(2); // empty line + separator
        }
        offsets.push(cur.min(u16::MAX as u32) as u16);
        cur = cur.saturating_add(file_visual_rows(update, diff_width));
    }
    offsets
}

fn content_total_rows(state: &State, diff_width: u16) -> u16 {
    let mut total: u32 = 0;
    for (i, update) in state.iter_ordered().enumerate() {
        if i > 0 {
            total = total.saturating_add(2);
        }
        total = total.saturating_add(file_visual_rows(update, diff_width));
    }
    total.min(u16::MAX as u32) as u16
}

fn file_visual_rows(update: &DiffUpdate, width: u16) -> u32 {
    let mut n: u32 = 1; // header line (assumed to fit)
    if update.binary {
        return n + 1;
    }
    if update.hunks.is_empty() && !matches!(update.status, ChangeKind::Deleted) {
        return n + 1;
    }
    for hunk in &update.hunks {
        n = n.saturating_add(1); // @@ header
        for line in &hunk.lines {
            let (content, prefix_len) = match line {
                HunkLine::Context(s) => (s.as_str(), 4), // "    "
                HunkLine::Added(s) => (s.as_str(), 4),   // "  + "
                HunkLine::Removed(s) => (s.as_str(), 4), // "  - "
            };
            n = n.saturating_add(line_visual_rows(content.chars().count() + prefix_len, width));
        }
    }
    n
}

fn line_visual_rows(content_chars: usize, width: u16) -> u32 {
    let w = width.max(1) as u32;
    let len = content_chars as u32;
    if len == 0 {
        return 1;
    }
    len.div_ceil(w)
}

fn diff_inner_width(area_width: u16, sidebar_visible: bool) -> u16 {
    let diff_outer = if sidebar_visible {
        let sb = sidebar_width(area_width);
        area_width.saturating_sub(sb)
    } else {
        area_width
    };
    diff_outer.saturating_sub(2) // block borders
}

fn make_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend).map_err(|e| Error::Term { source: e })
}

fn draw(
    frame: &mut ratatui::Frame,
    state: &State,
    scroll: u16,
    sidebar_visible: bool,
    repo_name: &str,
) {
    let area = frame.area();
    let footer_h = 1u16;
    let main = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(footer_h),
    };

    let diff_w = diff_inner_width(main.width, sidebar_visible);
    let offsets = file_offsets(state, diff_w);
    let current_idx = offsets.iter().rposition(|&o| o <= scroll).unwrap_or(0);
    let focused_path = state
        .iter_ordered()
        .nth(current_idx)
        .map(|u| u.path.display().to_string());

    if sidebar_visible {
        let sb_w = sidebar_width(main.width);
        let sidebar_area = Rect {
            x: main.x,
            y: main.y,
            width: sb_w,
            height: main.height,
        };
        let diff_area = Rect {
            x: main.x + sb_w,
            y: main.y,
            width: main.width.saturating_sub(sb_w),
            height: main.height,
        };
        draw_sidebar(frame, sidebar_area, state, current_idx);
        draw_diff(frame, diff_area, state, scroll, repo_name, focused_path.as_deref());
    } else {
        draw_diff(frame, main, state, scroll, repo_name, focused_path.as_deref());
    }

    let footer = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(footer_h),
        width: area.width,
        height: footer_h,
    };
    let hint = Paragraph::new(Line::from(vec![Span::styled(
        " q quit · j/k line · n/b file · g top · s sidebar · PgUp/PgDn page ",
        Style::default().fg(Color::DarkGray),
    )]));
    frame.render_widget(hint, footer);
}

fn sidebar_width(total: u16) -> u16 {
    let proposed = (u32::from(total) * 25 / 100) as u16;
    proposed.clamp(18, 32).min(total.saturating_sub(20))
}

fn draw_sidebar(frame: &mut ratatui::Frame, area: Rect, state: &State, current_idx: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" files ({}) ", state.len()))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.is_empty() {
        let para = Paragraph::new(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(para, inner);
        return;
    }

    let visible_files = ((inner.height as usize) / 2).max(1);
    let total = state.len();
    let sb_scroll = sidebar_scroll(current_idx, visible_files, total);

    let lines: Vec<Line<'static>> = state
        .iter_ordered()
        .enumerate()
        .skip(sb_scroll)
        .take(visible_files)
        .flat_map(|(idx, update)| sidebar_row(update, idx == current_idx, inner.width))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn sidebar_scroll(highlight: usize, visible: usize, total: usize) -> usize {
    if visible == 0 || total <= visible {
        return 0;
    }
    if highlight < visible / 2 {
        return 0;
    }
    if highlight >= total.saturating_sub(visible / 2) {
        return total - visible;
    }
    highlight.saturating_sub(visible / 2)
}

fn sidebar_row(update: &DiffUpdate, highlighted: bool, width: u16) -> Vec<Line<'static>> {
    let (badge, color) = match &update.status {
        ChangeKind::Added => ("A", Color::Green),
        ChangeKind::Modified => ("M", Color::Yellow),
        ChangeKind::Deleted => ("D", Color::Red),
        ChangeKind::Untracked => ("?", Color::Cyan),
        ChangeKind::Renamed { .. } => ("R", Color::Magenta),
    };
    let path_str = update.path.display().to_string();
    let counts = format!("+{} -{}", update.added, update.removed);

    let cursor = if highlighted { "▎" } else { " " };
    let cursor_style = if highlighted {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let badge_text = format!(" {} ", badge);
    let badge_style = Style::default()
        .bg(color)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let path_style = if highlighted {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let counts_style = Style::default().fg(Color::DarkGray);

    // Line 1: cursor(1) + badge(3) + space(1) + path
    let prefix_len = cursor.chars().count() + badge_text.chars().count() + 1;
    let path_max = (width as usize).saturating_sub(prefix_len);
    let path_truncated = truncate_left(&path_str, path_max);

    let line1 = Line::from(vec![
        Span::styled(cursor.to_string(), cursor_style),
        Span::styled(badge_text, badge_style),
        Span::raw(" "),
        Span::styled(path_truncated, path_style),
    ]);

    // Line 2: cursor(1) + 4-space indent + counts
    let line2 = Line::from(vec![
        Span::styled(cursor.to_string(), cursor_style),
        Span::raw("    "),
        Span::styled(counts, counts_style),
    ]);

    vec![line1, line2]
}

fn truncate_left(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    if max == 1 {
        return "…".to_string();
    }
    let take = max - 1;
    let start = chars.len() - take;
    let mut out = String::from("…");
    out.extend(chars[start..].iter());
    out
}

fn draw_diff(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &State,
    scroll: u16,
    repo_name: &str,
    focused: Option<&str>,
) {
    let inner_width = area.width.saturating_sub(2);
    let lines = render_lines(state, inner_width);
    let title = match focused {
        Some(p) if !p.is_empty() => format!(" {} · {} ", repo_name, p),
        _ => format!(" {} ", repo_name),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::DarkGray));
    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, area);
}

fn render_lines(state: &State, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if state.is_empty() {
        out.push(Line::from(Span::styled(
            "  (no changes — waiting for edits)",
            Style::default().fg(Color::DarkGray),
        )));
        return out;
    }
    for (i, update) in state.iter_ordered().enumerate() {
        if i > 0 {
            out.push(Line::from(""));
            out.push(separator_line(width));
        }
        out.extend(render_file(update));
    }
    out
}

fn separator_line(width: u16) -> Line<'static> {
    let n = width.max(1) as usize;
    Line::from(Span::styled(
        "─".repeat(n),
        Style::default().fg(Color::DarkGray),
    ))
}

fn render_file(update: &DiffUpdate) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let (label, color) = match &update.status {
        ChangeKind::Added => (" ADDED ", Color::Green),
        ChangeKind::Modified => (" MODIFIED ", Color::Yellow),
        ChangeKind::Deleted => (" DELETED ", Color::Red),
        ChangeKind::Untracked => (" UNTRACKED ", Color::Cyan),
        ChangeKind::Renamed { .. } => (" RENAMED ", Color::Magenta),
    };
    let path_display = match &update.status {
        ChangeKind::Renamed { from } => format!("{} → {}", from.display(), update.path.display()),
        _ => update.path.display().to_string(),
    };
    let summary = format!("  +{} -{}", update.added, update.removed);
    out.push(Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .bg(color)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(path_display, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(summary, Style::default().fg(Color::DarkGray)),
    ]));

    if update.binary {
        out.push(Line::from(Span::styled(
            "  (binary — diff suppressed)",
            Style::default().fg(Color::DarkGray),
        )));
        return out;
    }

    if update.hunks.is_empty() && !matches!(update.status, ChangeKind::Deleted) {
        out.push(Line::from(Span::styled(
            "  (no textual change)",
            Style::default().fg(Color::DarkGray),
        )));
        return out;
    }

    for hunk in &update.hunks {
        out.push(Line::from(Span::styled(
            format!(
                "  @@ -{},{} +{},{} @@",
                hunk.old_range.0 + 1,
                hunk.old_range.1,
                hunk.new_range.0 + 1,
                hunk.new_range.1
            ),
            Style::default().fg(Color::Cyan),
        )));
        for line in &hunk.lines {
            match line {
                HunkLine::Context(s) => out.push(Line::from(Span::raw(format!("    {}", s)))),
                HunkLine::Added(s) => out.push(Line::from(Span::styled(
                    format!("  + {}", s),
                    Style::default().fg(Color::Green),
                ))),
                HunkLine::Removed(s) => out.push(Line::from(Span::styled(
                    format!("  - {}", s),
                    Style::default().fg(Color::Red),
                ))),
            }
        }
    }
    out
}

struct TerminalGuard {
    _private: (),
}

static GUARD_INSTALLED: OnceLock<Mutex<bool>> = OnceLock::new();

impl TerminalGuard {
    fn install() -> Result<Self> {
        let installed = GUARD_INSTALLED.get_or_init(|| Mutex::new(false));
        let mut flag = installed.lock().expect("guard mutex");
        if *flag {
            return Ok(TerminalGuard { _private: () });
        }
        enable_raw_mode().map_err(|e| Error::Term { source: e })?;
        execute!(io::stdout(), EnterAlternateScreen).map_err(|e| Error::Term { source: e })?;
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            prev_hook(info);
        }));
        *flag = true;
        Ok(TerminalGuard { _private: () })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[allow(dead_code)]
fn select_recv_timeout<T>(rx: &Receiver<T>, timeout: Duration) -> std::result::Result<T, RecvTimeoutError> {
    rx.recv_timeout(timeout)
}
