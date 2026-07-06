# day-counter

`day-counter` is a Rust terminal app that counts how much time you spent in each
country during one calendar year from a list of flights. It accounts for local
departure and arrival times by converting every timestamp through an explicit
IANA time zone, then displays monthly and yearly percentages in a Ratatui
dashboard.

## Input format

Provide a CSV file with these headers:

```csv
departure_country,departure_timezone,departure_local,arrival_country,arrival_timezone,arrival_local
France,Europe/Paris,2026-01-12T09:30,Japan,Asia/Tokyo,2026-01-12T19:10
Japan,Asia/Tokyo,2026-02-04T23:55,United States,America/Los_Angeles,2026-02-04T16:20
```

Timestamps are local wall-clock times. Accepted timestamp formats are
`YYYY-MM-DDTHH:MM`, `YYYY-MM-DD HH:MM`, and the same forms with seconds.

IANA time zones are required for accuracy. Country names alone are ambiguous for
countries with multiple time zones, and offsets alone are not enough to handle
DST transitions correctly.

## Counting rules

- The report is limited to one calendar year, January 1 through December 31.
- The initial country owns time from local January 1 00:00 until the first
  flight departure.
- A country owns time from arrival local time until the next departure local
  time.
- Flight duration is shown as `In transit` by default, using the departure time
  zone for monthly grouping.
- Pass `--no-transit` to drop flight duration from the report.
- Ambiguous or nonexistent local times during DST transitions are rejected so the
  input can be corrected explicitly.

## Run

```sh
cargo run -- \
  --flights examples/flights.csv \
  --year 2026 \
  --initial-country France \
  --initial-timezone Europe/Paris
```

In a normal terminal this opens the Ratatui dashboard. Press `q` or `Esc` to
quit.

For text output:

```sh
cargo run -- \
  --flights examples/flights.csv \
  --year 2026 \
  --initial-country France \
  --initial-timezone Europe/Paris \
  --summary
```

By default, the current year is counted until now and past years are counted as a
full calendar year. Use `--full-year` to force a complete year, or `--as-of` to
choose an exact ending instant:

```sh
cargo run -- \
  --flights examples/flights.csv \
  --year 2026 \
  --initial-country France \
  --initial-timezone Europe/Paris \
  --as-of 2026-07-03T18:00 \
  --as-of-timezone Europe/Paris
```

`--as-of` also accepts RFC3339 timestamps with offsets, such as
`2026-07-03T18:00:00+02:00`.
