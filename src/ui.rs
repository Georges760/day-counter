use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use day_counter::{MONTHS, Report, percentage, seconds_to_days};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

pub fn run<B: Backend>(terminal: &mut Terminal<B>, report: &Report) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, report))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw(frame: &mut Frame<'_>, report: &Report) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(11),
            Constraint::Min(12),
        ])
        .split(frame.area());

    render_header(frame, root[0], report);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(root[1]);
    render_totals(frame, top[0], report);
    render_annual_graph(frame, top[1], report);
    render_monthly_graph(frame, root[2], report);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, report: &Report) {
    let transit = if report.include_transit {
        "transit shown"
    } else {
        "transit excluded"
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" day-counter {}", report.year),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  |  {:.2} tracked days  |  {}  |  q/Esc quit",
            seconds_to_days(report.total_seconds),
            transit
        )),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, area);
}

fn render_totals(frame: &mut Frame<'_>, area: Rect, report: &Report) {
    let rows = report.entries.iter().enumerate().map(|(index, entry)| {
        Row::new(vec![
            Cell::from(entry.country.clone()).style(Style::default().fg(color_for(index))),
            Cell::from(format!("{:.2}", entry.total_days())),
            Cell::from(format!("{:.0}", entry.total_seconds() as f64 / 3_600.0)),
            Cell::from(format!(
                "{:.1}%",
                percentage(entry.total_seconds(), report.total_seconds)
            )),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["Country", "Days", "Hours", "Year %"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .title(" Year Totals ")
            .borders(Borders::ALL),
    );

    frame.render_widget(table, area);
}

fn render_annual_graph(frame: &mut Frame<'_>, area: Rect, report: &Report) {
    let bar_width = area.width.saturating_sub(4) as usize;
    let buckets = report
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (index, entry.total_seconds()))
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    lines.push(Line::from(stacked_bar_spans(
        bar_width,
        &buckets,
        report.total_seconds,
    )));
    lines.push(Line::raw(""));

    for (index, entry) in report.entries.iter().take(6).enumerate() {
        lines.push(Line::from(vec![
            Span::styled("██ ", Style::default().fg(color_for(index))),
            Span::raw(format!(
                "{}  {:.2}d  {:.1}%",
                entry.country,
                entry.total_days(),
                percentage(entry.total_seconds(), report.total_seconds)
            )),
        ]));
    }

    if report.entries.len() > 6 {
        lines.push(Line::raw(format!(
            "+ {} more countries",
            report.entries.len() - 6
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Annual Share ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_monthly_graph(frame: &mut Frame<'_>, area: Rect, report: &Report) {
    let bar_width = area.width.saturating_sub(42).clamp(12, 42) as usize;
    let mut lines = Vec::new();

    for (month_index, month_name) in MONTHS.iter().enumerate() {
        let total = report.month_totals[month_index];
        let buckets = report
            .entries
            .iter()
            .enumerate()
            .filter_map(|(entry_index, entry)| {
                let seconds = entry.seconds_by_month[month_index];
                (seconds > 0).then_some((entry_index, seconds))
            })
            .collect::<Vec<_>>();

        let mut spans = vec![Span::styled(
            format!("{month_name:<3} "),
            Style::default().add_modifier(Modifier::BOLD),
        )];
        spans.extend(stacked_bar_spans(bar_width, &buckets, total));
        spans.push(Span::raw(format!(" {:>6.2}d ", seconds_to_days(total))));
        spans.push(Span::raw(month_detail(report, month_index, total)));
        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Monthly Days And Percentages ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn month_detail(report: &Report, month_index: usize, month_total: i64) -> String {
    if month_total <= 0 {
        return String::new();
    }

    report
        .entries
        .iter()
        .filter_map(|entry| {
            let seconds = entry.seconds_by_month[month_index];
            (seconds > 0).then(|| {
                format!(
                    "{} {:.1}d {:.0}%",
                    compact_country(&entry.country),
                    seconds_to_days(seconds),
                    percentage(seconds, month_total)
                )
            })
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn compact_country(country: &str) -> String {
    const MAX_LEN: usize = 14;
    if country.chars().count() <= MAX_LEN {
        return country.to_string();
    }

    let mut value = country.chars().take(MAX_LEN - 1).collect::<String>();
    value.push_str("...");
    value
}

fn stacked_bar_spans(width: usize, buckets: &[(usize, i64)], total: i64) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    if total <= 0 {
        return vec![Span::styled(
            "░".repeat(width),
            Style::default().fg(Color::DarkGray),
        )];
    }

    let mut segments = buckets
        .iter()
        .filter(|(_, seconds)| *seconds > 0)
        .map(|(index, seconds)| {
            let exact = *seconds as f64 * width as f64 / total as f64;
            (*index, *seconds, exact.floor() as usize, exact.fract())
        })
        .collect::<Vec<_>>();

    let mut used = segments
        .iter()
        .map(|(_, _, cells, _)| *cells)
        .sum::<usize>();
    for segment in &mut segments {
        if segment.2 == 0 && used < width {
            segment.2 = 1;
            used += 1;
        }
    }

    segments.sort_by(|left, right| {
        right
            .3
            .partial_cmp(&left.3)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for segment in &mut segments {
        if used >= width {
            break;
        }
        segment.2 += 1;
        used += 1;
    }
    segments.sort_by_key(|(index, _, _, _)| *index);

    let mut spans = Vec::new();
    let mut emitted = 0;
    for (index, _, cells, _) in segments {
        if emitted >= width {
            break;
        }
        let cells = cells.min(width - emitted);
        if cells > 0 {
            spans.push(Span::styled(
                "█".repeat(cells),
                Style::default().fg(color_for(index)),
            ));
            emitted += cells;
        }
    }

    if emitted < width {
        spans.push(Span::styled(
            "░".repeat(width - emitted),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans
}

fn color_for(index: usize) -> Color {
    const COLORS: [Color; 12] = [
        Color::Cyan,
        Color::Yellow,
        Color::Green,
        Color::Magenta,
        Color::LightBlue,
        Color::LightRed,
        Color::LightGreen,
        Color::LightMagenta,
        Color::LightCyan,
        Color::LightYellow,
        Color::Blue,
        Color::Gray,
    ];
    COLORS[index % COLORS.len()]
}
