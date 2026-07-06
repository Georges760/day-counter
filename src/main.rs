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
    EndMode, Location, MONTHS, Report, ReportOptions, load_flights_csv, parse_as_of,
    parse_timezone, percentage, seconds_to_days,
};
use ratatui::{Terminal, backend::CrosstermBackend};

#[derive(Debug, Parser)]
#[command(
    name = "day-counter",
    about = "Count country presence by hour from local-time flight records"
)]
struct Cli {
    /// CSV file with departure/arrival countries, IANA time zones, and local timestamps.
    #[arg(short, long)]
    flights: PathBuf,

    /// Calendar year to report. Defaults to the current UTC year.
    #[arg(short, long)]
    year: Option<i32>,

    /// Country where the year starts at local Jan 1 00:00.
    #[arg(long)]
    initial_country: String,

    /// IANA time zone for the initial country, for example Europe/Paris.
    #[arg(long)]
    initial_timezone: String,

    /// End the report at this instant. Accepts RFC3339 with offset or local time with --as-of-timezone.
    #[arg(long)]
    as_of: Option<String>,

    /// IANA time zone used when --as-of has no offset.
    #[arg(long)]
    as_of_timezone: Option<String>,

    /// Count the complete calendar year instead of year-to-date.
    #[arg(long)]
    full_year: bool,

    /// Drop flight duration instead of displaying it as an "In transit" bucket.
    #[arg(long)]
    no_transit: bool,

    /// Print a text summary instead of opening the Ratatui dashboard.
    #[arg(long)]
    summary: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let year = cli.year.unwrap_or_else(|| Utc::now().year());
    let initial_timezone = parse_timezone(&cli.initial_timezone)?;
    let as_of_timezone = cli
        .as_of_timezone
        .as_deref()
        .map(parse_timezone)
        .transpose()?;

    let end_mode = if cli.full_year {
        EndMode::FullYear
    } else if let Some(as_of) = cli.as_of.as_deref() {
        EndMode::Until(parse_as_of(as_of, as_of_timezone)?)
    } else if year == Utc::now().year() {
        EndMode::Until(Utc::now())
    } else {
        EndMode::FullYear
    };

    let flights = load_flights_csv(&cli.flights)?;
    let report = day_counter::build_report(
        flights,
        ReportOptions {
            year,
            initial_location: Location::new(cli.initial_country, initial_timezone),
            end_mode,
            include_transit: !cli.no_transit,
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
