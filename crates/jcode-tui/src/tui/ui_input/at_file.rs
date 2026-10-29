// CUSTOM (dis446 fork, ledger C1): `@file` completion popup. Floats like the
// command palette (no layout height). Rows never wrap: truncated by width.
use super::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const VISIBLE_ROWS: usize = 5;

fn truncate_width(text: &str, max: usize) -> String {
    if UnicodeWidthStr::width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn window_start(selected: usize, count: usize) -> usize {
    if count <= VISIBLE_ROWS {
        0
    } else {
        (selected + 1)
            .saturating_sub(VISIBLE_ROWS)
            .min(count - VISIBLE_ROWS)
    }
}

pub(in crate::tui::ui) fn at_file_lines(
    view: &crate::tui::AtFileView,
    width: usize,
) -> Vec<Line<'static>> {
    let start = window_start(view.selected, view.rows.len());
    let mut lines = Vec::new();
    for (i, (name, path, is_dir)) in view
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .take(VISIBLE_ROWS)
    {
        let selected = i == view.selected;
        let marker = if selected { "▸ " } else { "  " };
        let label = if *is_dir && !name.ends_with('/') {
            format!("{name}/")
        } else {
            name.clone()
        };
        let label_style = if selected {
            Style::default().fg(rgb(255, 213, 128))
        } else {
            Style::default().fg(rgb(128, 203, 196))
        };
        let budget = width.saturating_sub(2);
        let label = truncate_width(&label, budget);
        let remaining = budget.saturating_sub(UnicodeWidthStr::width(label.as_str()) + 2);
        let mut spans = vec![
            Span::styled(marker.to_string(), label_style),
            Span::styled(label, label_style),
        ];
        if remaining >= 4 {
            spans.push(Span::styled(
                format!("  {}", truncate_width(path, remaining)),
                Style::default().fg(dim_color()),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

pub(in crate::tui::ui) fn draw_at_file_overlay(frame: &mut Frame, app: &dyn TuiState, area: Rect) {
    let Some(view) = app.at_file_view() else {
        return;
    };
    let mut lines = at_file_lines(&view, area.width as usize);
    let Some(rect) = command_suggestions_overlay_rect(area, lines.len() as u16, frame.area())
    else {
        return;
    };
    lines.truncate(rect.height as usize);
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(lines), rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(n: usize, selected: usize) -> crate::tui::AtFileView {
        crate::tui::AtFileView {
            rows: (0..n)
                .map(|i| (format!("file{i}.rs"), format!("src/file{i}.rs"), false))
                .collect(),
            selected,
        }
    }

    #[test]
    fn shows_five_rows_and_scrolls() {
        assert_eq!(at_file_lines(&view(12, 0), 60).len(), 5);
        assert_eq!(window_start(0, 12), 0);
        assert_eq!(window_start(4, 12), 0);
        assert_eq!(window_start(5, 12), 1);
        assert_eq!(window_start(11, 12), 7);
    }

    #[test]
    fn rows_never_exceed_width() {
        for width in [8usize, 12, 20, 40] {
            for line in at_file_lines(&view(3, 1), width) {
                let w: usize = line.spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
                assert!(w <= width, "row width {w} > {width}");
            }
        }
    }

    #[test]
    fn dirs_get_slash() {
        let v = crate::tui::AtFileView {
            rows: vec![("src".into(), "src/".into(), true)],
            selected: 0,
        };
        let text: String = at_file_lines(&v, 40)[0].spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("src/"));
    }
}
