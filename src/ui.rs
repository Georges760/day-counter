//! The blit dashboard.
//!
//! Every string is formatted once in [`Dashboard::new`] and borrowed by the frame
//! afterwards. That is not only cheaper than the old four-redraws-a-second loop: blit's
//! `Text::rich` takes a borrowed `&[Span]`, so the text it points at has to outlive the
//! node that renders it, and owning it in the dashboard is what makes that work.

use std::rc::Rc;

use anyhow::Result;
use blit::{Input, Key, Sense, Sides, Sizing, WidgetId, state::Open};
use blit_tui::{
    Ui,
    atom::Border,
    color::Color,
    layout::{Align, flex},
    text::{HorizontalAlign, Span, TextAttributes, TextOptions, TextOverflow},
    widget::{Block, Text, Title, scroll_list},
};
use day_counter::{MONTHS, Report, percentage, seconds_to_days};

use crate::bar::{BarData, StackedBar, color_for};

/// Column a scroll list keeps for its scrollbar.
const GUTTER: f32 = 1.0;

const HEADINGS: [&str; 4] = ["Country", "Days", "Hours", "Year %"];
const COLUMNS: [Sizing; 4] = [
    Sizing::grow_range(14.0, f32::INFINITY),
    Sizing::fixed(9.0),
    Sizing::fixed(8.0),
    Sizing::fixed(8.0),
];

pub fn run(report: &Report) -> Result<()> {
    let mut dashboard = Dashboard::new(report);
    blit_tui::run(|ui| dashboard.render(ui))?;
    Ok(())
}

/// The dashboard's text, pre-rendered, plus the little interaction state it keeps.
struct Dashboard {
    title: String,
    meta: String,
    totals: Vec<TotalRow>,
    annual: Rc<BarData>,
    legend: Vec<String>,
    months: Vec<MonthRow>,
    countries: scroll_list::State,
    shares: scroll_list::State,
    calendar: scroll_list::State,
    /// Country under the pointer, recomputed every frame.
    hovered: Option<usize>,
    /// Country clicked in the totals list; survives the pointer leaving.
    pinned: Option<usize>,
}

struct TotalRow {
    country: String,
    days: String,
    hours: String,
    share: String,
}

struct MonthRow {
    label: String,
    bar: Rc<BarData>,
    days: String,
    detail: String,
}

impl Dashboard {
    fn new(report: &Report) -> Self {
        let transit = if report.include_transit {
            "transit shown"
        } else {
            "transit excluded"
        };

        let totals = report
            .entries
            .iter()
            .map(|entry| TotalRow {
                country: entry.country.clone(),
                days: format!("{:.2}", entry.total_days()),
                hours: format!("{:.0}", entry.total_seconds() as f64 / 3_600.0),
                share: format!(
                    "{:.1}%",
                    percentage(entry.total_seconds(), report.total_seconds)
                ),
            })
            .collect();

        let legend = report
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{}  {:.2}d  {:.1}%",
                    entry.country,
                    entry.total_days(),
                    percentage(entry.total_seconds(), report.total_seconds)
                )
            })
            .collect();

        // Months with nothing tracked carry no information, so they are left out rather
        // than drawn as an empty grey bar: everything before `--start-on`, and everything
        // after today when the current year is counted until now.
        let months = MONTHS
            .iter()
            .enumerate()
            .filter(|(month, _)| report.month_totals[*month] > 0)
            .map(|(month, name)| {
                let total = report.month_totals[month];
                MonthRow {
                    label: format!("{name:<3} "),
                    bar: Rc::new(BarData {
                        weights: report
                            .entries
                            .iter()
                            .map(|entry| entry.seconds_by_month[month])
                            .collect(),
                        total,
                    }),
                    days: format!(" {:>6.2}d ", seconds_to_days(total)),
                    detail: month_detail(report, month, total),
                }
            })
            .collect();

        Self {
            title: format!(" day-counter {}", report.year),
            meta: format!(
                "  |  {:.2} tracked days  |  {}  |  q/Esc quit",
                seconds_to_days(report.total_seconds),
                transit
            ),
            totals,
            annual: Rc::new(BarData {
                weights: report
                    .entries
                    .iter()
                    .map(|entry| entry.total_seconds())
                    .collect(),
                total: report.total_seconds,
            }),
            legend,
            months,
            countries: scroll_list::State::default(),
            shares: scroll_list::State::default(),
            calendar: scroll_list::State::default(),
            hovered: None,
            pinned: None,
        }
    }

    fn render(&mut self, mut ui: Ui<'_>) {
        if quit_requested(ui.input()) {
            ui.context().quit();
            return;
        }

        let mut root = ui.layout(flex::column());

        let quit = root
            .child().item(flex::item().height(Sizing::fixed(3.0)))
            .build(|ui: Ui<'_>| self.render_header(ui));

        {
            let mut middle = root
                .child().item(flex::item().height(Sizing::fixed(11.0)))
                .layout(flex::row());
            middle
                .child().item(flex::item().width(Sizing::percent(0.52)))
                .build(|ui: Ui<'_>| self.render_totals(ui));
            // Read the hover back after the list has run, so the bars pick it up in
            // this frame rather than the next one.
            let highlight = self.pinned.or(self.hovered);
            middle
                .child().item(flex::item().width(Sizing::percent(0.48)))
                .build(|ui: Ui<'_>| self.render_annual(ui, highlight));
        }

        let highlight = self.pinned.or(self.hovered);
        // `grow`, not `grow_range(12, ..)`: ratatui's `Min(12)` yields when the terminal is
        // short, while a hard floor here pushes the panel off screen and makes the month
        // list believe it has room for all twelve rows, so it never scrolls.
        root.child().item(flex::item().height(Sizing::grow()))
            .build(|ui: Ui<'_>| self.render_monthly(ui, highlight));

        if quit {
            root.context().quit();
        }
    }

    fn render_header(&self, ui: Ui<'_>) -> bool {
        let mut header = ui.layout(flex::row().padding(Sides::all(1.0)).align(Align::Center));
        header.insert(Block::new().border(Border::new(Color::Reset)));

        let spans = [
            Span::new(self.title.as_str())
                .color(Color::WHITE)
                .attributes(TextAttributes::BOLD),
            Span::new(self.meta.as_str()),
        ];
        header
            .child().item(line(Sizing::grow()))
            .insert(Text::rich(&spans).options(one_line()));

        header
            .child().item(flex::item().fixed(8.0, 1.0))
            .build(|mut ui: Ui<'_>| {
                let interaction = ui.interact(Sense::CLICK);

                ui.insert(Block::new().background(if interaction.hovered {
                    Color::LIGHT_RED
                } else {
                    Color::DARK_GRAY
                }));
                ui.insert(
                    Text::new("quit")
                        .options(one_line().horizontal_align(HorizontalAlign::Center)),
                );

                interaction.clicked
            })
    }

    fn render_totals(&mut self, ui: Ui<'_>) {
        let Self {
            totals,
            countries,
            hovered,
            pinned,
            ..
        } = self;
        let mut panel = panel(ui, " Year Totals ");

        {
            // The rows below live in a scroll list, which keeps a column for its
            // scrollbar; reserve the same column here so the headings line up. It is
            // padding rather than a spacer child, which would also cost a gap.
            let mut head = panel.child().item(line(Sizing::grow())).layout(
                flex::row()
                    .gap(1.0)
                    .padding(Sides::new().right(GUTTER)),
            );
            for (label, width) in HEADINGS.into_iter().zip(COLUMNS) {
                head.child()
                    .item(line(width))
                    .insert(Text::new(label).attributes(TextAttributes::BOLD));
            }
        }

        let mut picked = None;
        panel
            .child()
            .item(flex::item().grow())
            .build(scroll_list::new(
                countries,
                scroll_list::Config::new(1.0),
                totals.iter().enumerate(),
                |(index, _)| WidgetId::new(("country", *index)),
                |mut ui: Ui<'_>, (index, row): (usize, &TotalRow)| {
                    let interaction = ui.interact(Sense::CLICK);
                    if interaction.hovered {
                        picked = Some(index);
                    }
                    if interaction.clicked {
                        *pinned = (*pinned != Some(index)).then_some(index);
                    }

                    let mut node = ui.layout(flex::row().gap(1.0));
                    if *pinned == Some(index) || interaction.hovered {
                        node.insert(Block::new().background(Color::DARK_GRAY));
                    }
                    node.child().item(line(COLUMNS[0])).insert(
                        Text::new(row.country.as_str())
                            .color(color_for(index))
                            .options(one_line()),
                    );
                    node.child()
                        .item(line(COLUMNS[1]))
                        .insert(Text::new(row.days.as_str()));
                    node.child()
                        .item(line(COLUMNS[2]))
                        .insert(Text::new(row.hours.as_str()));
                    node.child()
                        .item(line(COLUMNS[3]))
                        .insert(Text::new(row.share.as_str()));
                },
                scrollbar,
            ));
        *hovered = picked;
    }

    fn render_annual(&mut self, ui: Ui<'_>, highlight: Option<usize>) {
        let Self {
            annual,
            legend,
            shares,
            ..
        } = self;
        let mut panel = panel(ui, " Annual Share ");

        {
            let mut row = panel.child().item(line(Sizing::grow())).layout(flex::row());
            row.child().item(line(Sizing::grow()))
                .insert(StackedBar::new(Rc::clone(annual)).highlight(highlight));
            // The old bar stopped two cells short of the inner width; keep that margin.
            row.child().item(line(Sizing::fixed(2.0))).insert(());
        }

        panel.child().item(line(Sizing::grow())).insert(());

        // Every country is listed; the ones that do not fit are scrolled to.
        panel
            .child()
            .item(flex::item().grow())
            .build(scroll_list::new(
                shares,
                scroll_list::Config::new(1.0),
                legend.iter().enumerate(),
                |(index, _)| WidgetId::new(("share", *index)),
                |mut ui: Ui<'_>, (index, entry): (usize, &String)| {
                    let spans = [
                        Span::new("██ ").color(color_for(index)),
                        Span::new(entry.as_str()),
                    ];
                    ui.insert(Text::rich(&spans).options(one_line()));
                },
                scrollbar,
            ));
    }

    fn render_monthly(&mut self, ui: Ui<'_>, highlight: Option<usize>) {
        // One bar width for all twelve months, taken from the current frame size, so the
        // bars stay comparable. A per-row `grow` would let one month's long detail line
        // shrink that month's bar alone.
        let bar_width = (ui.screen().width - 42.0).clamp(12.0, 42.0);
        let Self {
            months, calendar, ..
        } = self;
        let mut panel = panel(ui, " Monthly Days And Percentages ");

        panel
            .child()
            .item(flex::item().grow())
            .build(scroll_list::new(
                calendar,
                scroll_list::Config::new(1.0),
                months.iter(),
                |month| WidgetId::new(("month", month.label.as_str())),
                |ui: Ui<'_>, month: &MonthRow| {
                    let mut row = ui.layout(flex::row());
                    row.child()
                        .item(line(Sizing::fixed(4.0)))
                        .insert(Text::new(month.label.as_str()).attributes(TextAttributes::BOLD));
                    row.child()
                        .item(line(Sizing::fixed(bar_width)))
                        .insert(StackedBar::new(Rc::clone(&month.bar)).highlight(highlight));
                    row.child()
                        .item(line(Sizing::fixed(9.0)))
                        .insert(Text::new(month.days.as_str()));
                    row.child()
                        .item(line(Sizing::grow()))
                        .insert(Text::new(month.detail.as_str()).options(one_line()));
                },
                scrollbar,
            ));
    }
}

/// A bordered, titled box whose children sit inside the border.
fn panel<'a>(ui: Ui<'a>, title: &str) -> Ui<'a, Open<flex::Layout>> {
    let mut panel = ui.layout(flex::column().padding(Sides::all(1.0)));
    panel.insert(
        Block::new()
            .border(Border::new(Color::Reset))
            .title(Title::new(title)),
    );
    panel
}

/// A one-row flex child of the given width.
fn line(width: Sizing) -> flex::Item {
    flex::item().width(width).height(Sizing::fixed(1.0))
}

/// One row, clipped with an ellipsis rather than wrapped.
fn one_line() -> TextOptions {
    TextOptions::new()
        .overflow(TextOverflow::Ellipsis)
        .max_lines(1)
}

/// blit routes a bare letter as text and a modified one as a key, so `q` and `Ctrl+C`
/// arrive through different arms.
fn quit_requested(input: &Input) -> bool {
    match input {
        Input::Text('q') => true,
        Input::Key(key) if key.pressed => match key.key {
            Key::Escape | Key::Character('q') => true,
            Key::Character('c') => key.modifiers.control(),
            _ => false,
        },
        _ => false,
    }
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

/// A one-cell scrollbar: an invisible track with a thumb that brightens while dragged.
///
/// The track is kept even though it draws nothing: it is what makes the list reserve
/// its scrollbar column, so the thumb never covers the last character of a row.
fn scrollbar(active: bool) -> (Option<Block<'static>>, Option<Block<'static>>) {
    (
        Some(Block::new().background(Color::Reset)),
        Some(Block::new().background(if active {
            Color::WHITE
        } else {
            Color::DARK_GRAY
        })),
    )
}
