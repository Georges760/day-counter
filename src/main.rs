mod ui;

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{Datelike, Utc};
use clap::Parser;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use day_counter::{
    EndMode, MONTHS, Report, ReportOptions, TripInput, load_trip_json, percentage, seconds_to_days,
};
use ratatui::{Terminal, backend::CrosstermBackend};

#[derive(Debug, Parser)]
#[command(
    name = "day-counter",
    about = "Count country presence by hour from local-time flight records"
)]
struct Cli {
    /// JSON config file with report settings and flights.
    #[arg(short, long, alias = "input")]
    config: PathBuf,

    /// Print a text summary instead of opening the Ratatui dashboard.
    #[arg(long)]
    summary: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let TripInput {
        year,
        initial_location,
        full_year,
        include_transit,
        flights,
    } = load_trip_json(&cli.config)?;

    let end_mode = if full_year {
        EndMode::FullYear
    } else if year == Utc::now().year() {
        EndMode::Until(Utc::now())
    } else {
        EndMode::FullYear
    };

    let report = day_counter::build_report(
        flights,
        ReportOptions {
            year,
            initial_location,
            end_mode,
            include_transit,
        },
    )?;

    if cli.summary || !io::stdout().is_terminal() {
        print_summary(&report);
        return Ok(());
    }

    run_tui(&report).context("failed to run terminal UI")
}

fn run_tui(report: &Report) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = ui::run(&mut terminal, report);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn print_summary(report: &Report) {
    println!("day-counter {}", report.year);
    println!(
        "tracked: {:.2} days ({:.0} hours){}",
        seconds_to_days(report.total_seconds),
        report.total_seconds as f64 / 3_600.0,
        if report.include_transit {
            ""
        } else {
            ", transit excluded"
        }
    );
    println!();
    println!("{:<24} {:>10} {:>9}", "Country", "Days", "Year %");
    println!("{:-<45}", "");
    for entry in &report.entries {
        println!(
            "{:<24} {:>10.2} {:>8.1}%",
            entry.country,
            entry.total_days(),
            percentage(entry.total_seconds(), report.total_seconds)
        );
    }

    println!();
    println!("Monthly breakdown");
    for (month_index, month_name) in MONTHS.iter().enumerate() {
        let total = report.month_totals[month_index];
        if total == 0 {
            continue;
        }

        let values = report
            .entries
            .iter()
            .filter_map(|entry| {
                let seconds = entry.seconds_by_month[month_index];
                (seconds > 0).then(|| {
                    format!(
                        "{} {:.2}d {:.1}%",
                        entry.country,
                        seconds_to_days(seconds),
                        percentage(seconds, total)
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(" | ");
        println!("{month_name}: {values}");
    }
}
