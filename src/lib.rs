use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;

pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub const TRANSIT_BUCKET: &str = "In transit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub country: String,
    pub timezone: Tz,
}

impl Location {
    pub fn new(country: impl Into<String>, timezone: Tz) -> Self {
        Self {
            country: country.into(),
            timezone,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Flight {
    pub departure: FlightEndpoint,
    pub arrival: FlightEndpoint,
}

impl Flight {
    pub fn departure_utc(&self) -> DateTime<Utc> {
        self.departure.utc
    }

    pub fn arrival_utc(&self) -> DateTime<Utc> {
        self.arrival.utc
    }
}

#[derive(Debug, Clone)]
pub struct FlightEndpoint {
    pub location: Location,
    pub local: NaiveDateTime,
    pub utc: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum EndMode {
    Until(DateTime<Utc>),
    FullYear,
}

#[derive(Debug, Clone)]
pub struct ReportOptions {
    pub year: i32,
    pub initial_location: Location,
    pub end_mode: EndMode,
    pub include_transit: bool,
}

#[derive(Debug, Clone)]
pub struct TripInput {
    pub year: i32,
    pub initial_location: Location,
    pub full_year: bool,
    pub include_transit: bool,
    pub flights: Vec<Flight>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub year: i32,
    pub entries: Vec<CountryStats>,
    pub month_totals: [i64; 12],
    pub total_seconds: i64,
    pub include_transit: bool,
}

impl Report {
    pub fn country(&self, country: &str) -> Option<&CountryStats> {
        self.entries.iter().find(|entry| entry.country == country)
    }
}

#[derive(Debug, Clone)]
pub struct CountryStats {
    pub country: String,
    pub seconds_by_month: [i64; 12],
}

impl CountryStats {
    pub fn total_seconds(&self) -> i64 {
        self.seconds_by_month.iter().sum()
    }

    pub fn month_days(&self, month_index: usize) -> f64 {
        seconds_to_days(self.seconds_by_month[month_index])
    }

    pub fn total_days(&self) -> f64 {
        seconds_to_days(self.total_seconds())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonTrip {
    year: i32,
    #[serde(alias = "initial-country")]
    initial_country: String,
    #[serde(alias = "initial-timezone")]
    initial_timezone: String,
    #[serde(default, alias = "full-year")]
    full_year: bool,
    #[serde(default = "default_include_transit", alias = "include-transit")]
    include_transit: bool,
    flights: Vec<JsonFlight>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonFlight {
    #[serde(alias = "departure-country")]
    departure_country: String,
    #[serde(alias = "departure-timezone")]
    departure_timezone: String,
    #[serde(alias = "departure-local")]
    departure_local: String,
    #[serde(alias = "arrival-country")]
    arrival_country: String,
    #[serde(alias = "arrival-timezone")]
    arrival_timezone: String,
    #[serde(alias = "arrival-local")]
    arrival_local: String,
}

pub fn load_trip_json(path: impl AsRef<Path>) -> Result<TripInput> {
    let path = path.as_ref();
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_trip_json(&contents).with_context(|| format!("invalid JSON in {}", path.display()))
}

pub fn parse_trip_json(contents: &str) -> Result<TripInput> {
    let trip: JsonTrip = serde_json::from_str(contents)?;
    parse_json_trip(trip)
}

pub fn parse_timezone(value: &str) -> Result<Tz> {
    Tz::from_str(value.trim()).with_context(|| {
        format!(
            "invalid IANA time zone `{}`; use a value such as Europe/Paris or Asia/Tokyo",
            value
        )
    })
}

pub fn parse_local_datetime(value: &str) -> Result<NaiveDateTime> {
    let value = value.trim();
    let formats = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ];

    formats
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| {
            anyhow!(
                "invalid local timestamp `{}`; expected YYYY-MM-DDTHH:MM or YYYY-MM-DDTHH:MM:SS",
                value
            )
        })
}

pub fn local_datetime_to_utc(timezone: Tz, local: NaiveDateTime) -> Result<DateTime<Utc>> {
    match timezone.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, second) => bail!(
            "local timestamp {} is ambiguous in {}; use a non-ambiguous time outside the DST fold ({}/{})",
            local,
            timezone,
            first.format("%:z"),
            second.format("%:z")
        ),
        LocalResult::None => bail!(
            "local timestamp {} does not exist in {} because of a time-zone transition",
            local,
            timezone
        ),
    }
}

pub fn build_report(mut flights: Vec<Flight>, options: ReportOptions) -> Result<Report> {
    flights.sort_by_key(Flight::departure_utc);

    for flight in &flights {
        if flight.arrival_utc() < flight.departure_utc() {
            bail!(
                "flight from {} to {} arrives before it departs",
                flight.departure.location.country,
                flight.arrival.location.country
            );
        }
    }

    let mut accumulator = Accumulator::default();
    let mut current_location = options.initial_location.clone();
    let mut current_start = start_of_year_utc(options.year, current_location.timezone)?;
    let report_end = report_end_guard(options.year, &options.end_mode);

    if matches!(options.end_mode, EndMode::Until(end) if end <= current_start) {
        return Ok(accumulator.finish(options.year, options.include_transit));
    }

    for flight in flights {
        if flight.departure_utc() >= report_end {
            break;
        }

        if flight.arrival_utc() <= current_start {
            continue;
        }

        if flight.departure_utc() < current_start {
            bail!(
                "flight departing {} from {} overlaps the previous known position; check the flight order and timestamps",
                flight.departure.local,
                flight.departure.location.country
            );
        }

        accumulator.add_interval(
            &current_location,
            current_start,
            flight.departure_utc().min(report_end),
            options.year,
        )?;

        if options.include_transit {
            let transit_location =
                Location::new(TRANSIT_BUCKET, flight.departure.location.timezone);
            accumulator.add_interval(
                &transit_location,
                flight.departure_utc(),
                flight.arrival_utc().min(report_end),
                options.year,
            )?;
        }

        current_start = flight.arrival_utc();
        current_location = flight.arrival.location;
    }

    accumulator.add_interval(&current_location, current_start, report_end, options.year)?;

    Ok(accumulator.finish(options.year, options.include_transit))
}

pub fn seconds_to_days(seconds: i64) -> f64 {
    seconds as f64 / 86_400.0
}

pub fn percentage(part: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

fn default_include_transit() -> bool {
    true
}

fn parse_json_trip(trip: JsonTrip) -> Result<TripInput> {
    let initial_timezone = parse_timezone(&trip.initial_timezone)?;

    let mut flights = Vec::with_capacity(trip.flights.len());
    for (index, flight) in trip.flights.into_iter().enumerate() {
        flights.push(
            parse_json_flight(flight)
                .with_context(|| format!("invalid flight at index {}", index))?,
        );
    }

    Ok(TripInput {
        year: trip.year,
        initial_location: Location::new(trip.initial_country.trim(), initial_timezone),
        full_year: trip.full_year,
        include_transit: trip.include_transit,
        flights,
    })
}

fn parse_json_flight(row: JsonFlight) -> Result<Flight> {
    let departure_timezone = parse_timezone(&row.departure_timezone)?;
    let arrival_timezone = parse_timezone(&row.arrival_timezone)?;
    let departure_local = parse_local_datetime(&row.departure_local)?;
    let arrival_local = parse_local_datetime(&row.arrival_local)?;

    Ok(Flight {
        departure: FlightEndpoint {
            location: Location::new(row.departure_country.trim(), departure_timezone),
            local: departure_local,
            utc: local_datetime_to_utc(departure_timezone, departure_local)?,
        },
        arrival: FlightEndpoint {
            location: Location::new(row.arrival_country.trim(), arrival_timezone),
            local: arrival_local,
            utc: local_datetime_to_utc(arrival_timezone, arrival_local)?,
        },
    })
}

fn start_of_year_utc(year: i32, timezone: Tz) -> Result<DateTime<Utc>> {
    let local = NaiveDate::from_ymd_opt(year, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| anyhow!("invalid year {}", year))?;
    local_boundary_to_utc(timezone, local)
}

fn report_end_guard(year: i32, end_mode: &EndMode) -> DateTime<Utc> {
    match end_mode {
        EndMode::Until(value) => *value,
        EndMode::FullYear => Utc
            .with_ymd_and_hms(year + 1, 1, 2, 0, 0, 0)
            .single()
            .expect("valid UTC date"),
    }
}

fn local_boundary_to_utc(timezone: Tz, local: NaiveDateTime) -> Result<DateTime<Utc>> {
    match timezone.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, second) => Ok(first.min(second).with_timezone(&Utc)),
        LocalResult::None => {
            let mut probe = local;
            for _ in 0..180 {
                probe += Duration::minutes(1);
                match timezone.from_local_datetime(&probe) {
                    LocalResult::Single(value) => return Ok(value.with_timezone(&Utc)),
                    LocalResult::Ambiguous(first, second) => {
                        return Ok(first.min(second).with_timezone(&Utc));
                    }
                    LocalResult::None => {}
                }
            }
            bail!(
                "could not find a valid local boundary after {} in {}",
                local,
                timezone
            )
        }
    }
}

#[derive(Default)]
struct Accumulator {
    seconds_by_country: BTreeMap<String, [i64; 12]>,
}

impl Accumulator {
    fn add_interval(
        &mut self,
        location: &Location,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        year: i32,
    ) -> Result<()> {
        if end <= start {
            return Ok(());
        }

        let mut cursor = start;

        while cursor < end {
            let local_cursor = cursor.with_timezone(&location.timezone);
            let next_boundary =
                next_month_start_utc(location.timezone, local_cursor.year(), local_cursor.month())?;
            let segment_end = end.min(next_boundary);

            if local_cursor.year() == year {
                let month_index = local_cursor.month0() as usize;
                let seconds = segment_end
                    .signed_duration_since(cursor)
                    .num_seconds()
                    .max(0);
                self.seconds_by_country
                    .entry(location.country.clone())
                    .or_default()[month_index] += seconds;
            }

            if segment_end <= cursor {
                bail!(
                    "internal error while splitting interval for {} at {}",
                    location.country,
                    cursor
                );
            }
            cursor = segment_end;
        }

        Ok(())
    }

    fn finish(self, year: i32, include_transit: bool) -> Report {
        let mut entries: Vec<_> = self
            .seconds_by_country
            .into_iter()
            .map(|(country, seconds_by_month)| CountryStats {
                country,
                seconds_by_month,
            })
            .collect();
        entries.sort_by(|left, right| {
            right
                .total_seconds()
                .cmp(&left.total_seconds())
                .then_with(|| left.country.cmp(&right.country))
        });

        let mut month_totals = [0; 12];
        for entry in &entries {
            for (index, seconds) in entry.seconds_by_month.iter().enumerate() {
                month_totals[index] += seconds;
            }
        }

        let total_seconds = month_totals.iter().sum();

        Report {
            year,
            entries,
            month_totals,
            total_seconds,
            include_transit,
        }
    }
}

fn next_month_start_utc(timezone: Tz, year: i32, month: u32) -> Result<DateTime<Utc>> {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let local = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| anyhow!("invalid month boundary {}-{}", next_year, next_month))?;
    local_boundary_to_utc(timezone, local)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(country: &str, timezone: &str, local: &str) -> FlightEndpoint {
        let timezone = parse_timezone(timezone).unwrap();
        let local = parse_local_datetime(local).unwrap();
        FlightEndpoint {
            location: Location::new(country, timezone),
            local,
            utc: local_datetime_to_utc(timezone, local).unwrap(),
        }
    }

    fn flight(
        departure_country: &str,
        departure_timezone: &str,
        departure_local: &str,
        arrival_country: &str,
        arrival_timezone: &str,
        arrival_local: &str,
    ) -> Flight {
        Flight {
            departure: endpoint(departure_country, departure_timezone, departure_local),
            arrival: endpoint(arrival_country, arrival_timezone, arrival_local),
        }
    }

    #[test]
    fn counts_country_stays_and_transit_by_hour() {
        let report = build_report(
            vec![flight(
                "France",
                "Europe/Paris",
                "2026-01-01T10:00",
                "United Kingdom",
                "Europe/London",
                "2026-01-01T11:00",
            )],
            ReportOptions {
                year: 2026,
                initial_location: Location::new("France", parse_timezone("Europe/Paris").unwrap()),
                end_mode: EndMode::Until(
                    local_datetime_to_utc(
                        parse_timezone("Europe/London").unwrap(),
                        parse_local_datetime("2026-01-02T00:00").unwrap(),
                    )
                    .unwrap(),
                ),
                include_transit: true,
            },
        )
        .unwrap();

        assert_eq!(
            report.country("France").unwrap().seconds_by_month[0],
            36_000
        );
        assert_eq!(
            report.country(TRANSIT_BUCKET).unwrap().seconds_by_month[0],
            7_200
        );
        assert_eq!(
            report.country("United Kingdom").unwrap().seconds_by_month[0],
            46_800
        );
    }

    #[test]
    fn splits_stays_by_local_month() {
        let timezone = parse_timezone("Europe/Paris").unwrap();
        let mut accumulator = Accumulator::default();
        accumulator
            .add_interval(
                &Location::new("France", timezone),
                local_datetime_to_utc(timezone, parse_local_datetime("2026-01-31T23:00").unwrap())
                    .unwrap(),
                local_datetime_to_utc(timezone, parse_local_datetime("2026-02-01T01:00").unwrap())
                    .unwrap(),
                2026,
            )
            .unwrap();

        let report = accumulator.finish(2026, true);
        let france = report.country("France").unwrap();
        assert_eq!(france.seconds_by_month[0], 3_600);
        assert_eq!(france.seconds_by_month[1], 3_600);
    }

    #[test]
    fn rejects_ambiguous_dst_local_times() {
        let timezone = parse_timezone("Europe/Paris").unwrap();
        let local = parse_local_datetime("2026-10-25T02:30").unwrap();
        let error = local_datetime_to_utc(timezone, local).unwrap_err();
        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn parses_json_trip_with_hyphenated_aliases() {
        let trip = parse_trip_json(
            r#"
            {
              "year": 2026,
              "initial-country": "France",
              "initial-timezone": "Europe/Paris",
              "include-transit": false,
              "flights": [
                {
                  "departure-country": "France",
                  "departure-timezone": "Europe/Paris",
                  "departure-local": "2026-01-12T09:30",
                  "arrival-country": "Japan",
                  "arrival-timezone": "Asia/Tokyo",
                  "arrival-local": "2026-01-12T19:10"
                }
              ]
            }
            "#,
        )
        .unwrap();

        assert_eq!(trip.year, 2026);
        assert_eq!(trip.initial_location.country, "France");
        assert!(!trip.include_transit);
        assert_eq!(trip.flights.len(), 1);
        assert_eq!(trip.flights[0].arrival.location.country, "Japan");
    }
}
