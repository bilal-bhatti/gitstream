//! Frame drawing. Pure functions of `(state, scroll, focused_idx, geometry)`
//! returning ratatui widgets — no input handling, no scroll/focus mutation.

use crate::render::term::palette;
use crate::render::view::wrap_at;
use crate::state::{ChangeKind, DiffUpdate, HunkLine, State};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

pub fn draw(
    frame: &mut ratatui::Frame,
    state: &State,
    scroll: u16,
    focused_idx: usize,
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

    let current_idx = focused_idx;
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
        draw_diff(
            frame,
            diff_area,
            state,
            scroll,
            repo_name,
            focused_path.as_deref(),
        );
    } else {
        draw_diff(
            frame,
            main,
            state,
            scroll,
            repo_name,
            focused_path.as_deref(),
        );
    }

    let footer = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(footer_h),
        width: area.width,
        height: footer_h,
    };
    let hint = Paragraph::new(Line::from(vec![Span::styled(
        " q quit · j/k line · u/d or PgUp/PgDn page · n/b file · g top · s sidebar · e edit ",
        Style::default().fg(Color::DarkGray),
    )]));
    frame.render_widget(hint, footer);
}

pub fn diff_inner_width(area_width: u16, sidebar_visible: bool) -> u16 {
    let diff_outer = if sidebar_visible {
        let sb = sidebar_width(area_width);
        area_width.saturating_sub(sb)
    } else {
        area_width
    };
    diff_outer.saturating_sub(2) // block borders
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

    let visible_files = ((inner.height as usize) / 3).max(1);
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
        ChangeKind::Modified => ("M", Color::Yellow),
        ChangeKind::Deleted => ("D", Color::Red),
        ChangeKind::Untracked => ("?", Color::Cyan),
    };
    let path_str = update.path.display().to_string();
    let counts = format!("+{} -{}", update.added, update.removed);

    let cursor = if highlighted { "▎" } else { " " };
    let row_bg = if highlighted {
        Some(palette().row_bg)
    } else {
        None
    };
    let with_bg = |s: Style| match row_bg {
        Some(c) => s.bg(c),
        None => s,
    };

    let cursor_style = with_bg(if highlighted {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    });
    let badge_text = format!(" {} ", badge);
    let badge_style = Style::default()
        .bg(color)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let path_style = with_bg(if highlighted {
        Style::default()
            .fg(palette().highlight_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    });
    let counts_style = with_bg(Style::default().fg(Color::DarkGray));
    let pad_style = with_bg(Style::default());

    // Line 1: cursor(1) + badge(3) + space(1) + path. Use display width for
    // the prefix in case any glyph in cursor/badge is ever a wide char.
    let prefix_len =
        UnicodeWidthStr::width(cursor) + UnicodeWidthStr::width(badge_text.as_str()) + 1;
    let path_max = (width as usize).saturating_sub(prefix_len);
    let path_truncated = truncate_left(&path_str, path_max);
    let line1_used = prefix_len + UnicodeWidthStr::width(path_truncated.as_str());
    let line1_pad = (width as usize).saturating_sub(line1_used);

    let mut line1_spans = vec![
        Span::styled(cursor.to_string(), cursor_style),
        Span::styled(badge_text, badge_style),
        Span::styled(" ", pad_style),
        Span::styled(path_truncated, path_style),
    ];
    if highlighted && line1_pad > 0 {
        line1_spans.push(Span::styled(" ".repeat(line1_pad), pad_style));
    }

    // Line 2: cursor(1) + 4-space indent + counts
    let line2_used = 1 + 4 + UnicodeWidthStr::width(counts.as_str());
    let line2_pad = (width as usize).saturating_sub(line2_used);
    let mut line2_spans = vec![
        Span::styled(cursor.to_string(), cursor_style),
        Span::styled("    ", pad_style),
        Span::styled(counts, counts_style),
    ];
    if highlighted && line2_pad > 0 {
        line2_spans.push(Span::styled(" ".repeat(line2_pad), pad_style));
    }

    // Line 3: spacer between rows. On highlighted rows it continues the marker
    // and tinted background so the entry reads as one block; on others it's
    // empty whitespace, giving non-flush separation.
    let line3 = if highlighted {
        let pad = (width as usize).saturating_sub(1);
        Line::from(vec![
            Span::styled(cursor.to_string(), cursor_style),
            Span::styled(" ".repeat(pad), pad_style),
        ])
    } else {
        Line::from("")
    };

    vec![Line::from(line1_spans), Line::from(line2_spans), line3]
}

/// Truncate `s` from the left so its display width fits in `max` cells.
/// Uses unicode-width so CJK / wide emoji are counted as 2 cells, matching
/// what ratatui actually renders. Prefixes "…" if anything was dropped.
fn truncate_left(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    if max == 1 {
        return "…".to_string();
    }
    // We need room for the leading "…" (1 cell), so collect chars from the
    // end until we've filled (max - 1) cells of display width.
    let budget = max - 1;
    let mut tail: Vec<char> = Vec::new();
    let mut used: usize = 0;
    for ch in s.chars().rev() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        tail.push(ch);
    }
    let mut out = String::from("…");
    out.extend(tail.into_iter().rev());
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
    let version = format!(" {} ", crate::VERSION);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(title))
        .title(Line::from(version).right_aligned())
        .border_style(Style::default().fg(Color::DarkGray));
    // No ratatui Wrap — `render_lines` pre-wraps every diff line at exact
    // character boundaries so each emitted Line corresponds to exactly one
    // rendered row. WordWrapper's word-boundary breaking would silently
    // diverge from `view::file_visual_rows`, putting n/b's scroll target
    // at the wrong row for any content past a long-line file (minified JS,
    // generated code, etc.).
    let para = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(para, area);
}

pub fn render_lines(state: &State, width: u16) -> Vec<Line<'_>> {
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
        out.extend(render_file(update, width));
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

fn render_file(update: &DiffUpdate, width: u16) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let (label, color) = match &update.status {
        ChangeKind::Modified => (" MODIFIED ", Color::Yellow),
        ChangeKind::Deleted => (" DELETED ", Color::Red),
        ChangeKind::Untracked => (" UNTRACKED ", Color::Cyan),
    };
    let path_display = update.path.display().to_string();
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

    let content_w = width.saturating_sub(4);
    for hunk in &update.hunks {
        // Git uses `@@ -0,0 +1,N @@` for a new file (start is 0 when count is 0)
        // and `@@ -1,N +0,0 @@` for full deletion. Without the conditional we'd
        // render `-1,0` / `+1,0`, which is malformed.
        let old_start = if hunk.old_range.1 == 0 {
            0
        } else {
            hunk.old_range.0 + 1
        };
        let new_start = if hunk.new_range.1 == 0 {
            0
        } else {
            hunk.new_range.0 + 1
        };
        out.push(Line::from(Span::styled(
            format!(
                "  @@ -{},{} +{},{} @@",
                old_start, hunk.old_range.1, new_start, hunk.new_range.1
            ),
            Style::default().fg(Color::Cyan),
        )));
        // Pre-wrap each diff line at exact display-cell boundaries so the
        // emitted line count matches `view::file_visual_rows` byte-for-byte.
        // ratatui's WordWrapper would silently disagree on minified /
        // no-whitespace content.
        for line in &hunk.lines {
            match line {
                HunkLine::Context(s) => {
                    for chunk in wrap_at(s.as_str(), content_w) {
                        out.push(Line::from(vec![Span::raw("    "), Span::raw(chunk)]));
                    }
                }
                HunkLine::Added(s) => {
                    for chunk in wrap_at(s.as_str(), content_w) {
                        out.push(Line::from(vec![
                            Span::styled("  + ", Style::default().fg(Color::Green)),
                            Span::styled(chunk, Style::default().fg(Color::Green)),
                        ]));
                    }
                }
                HunkLine::Removed(s) => {
                    for chunk in wrap_at(s.as_str(), content_w) {
                        out.push(Line::from(vec![
                            Span::styled("  - ", Style::default().fg(Color::Red)),
                            Span::styled(chunk, Style::default().fg(Color::Red)),
                        ]));
                    }
                }
            }
        }
    }
    out
}
