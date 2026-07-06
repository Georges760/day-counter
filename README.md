# day-counter

`day-counter` is a Rust terminal app that counts how much time you spent in each
country during one calendar year from a list of flights. It accounts for local
departure and arrival times by converting every timestamp through an explicit
IANA time zone, then displays monthly and yearly percentages in a Ratatui
dashboard.

## Input Format

Provide a JSON file with the report settings and flight list:

```json
{
  "year": 2026,
  "initial_country": "France",
  "initial_timezone": "Europe/Paris",
  "include_transit": true,
  "flights": [
    {
      "departure_country": "France",
      "departure_timezone": "Europe/Paris",
      "departure_local": "2026-01-12T09:30",
      "arrival_country": "Japan",
      "arrival_timezone": "Asia/Tokyo",
      "arrival_local": "2026-01-12T19:10"
    },
    {
      "departure_country": "Japan",
      "departure_timezone": "Asia/Tokyo",
      "departure_local": "2026-02-04T23:55",
      "arrival_country": "United States",
      "arrival_timezone": "America/Los_Angeles",
      "arrival_local": "2026-02-04T16:20"
    }
  ]
}
```

Timestamps are local wall-clock times. Accepted timestamp formats are
`YYYY-MM-DDTHH:MM`, `YYYY-MM-DD HH:MM`, and the same forms with seconds.

JSON keys can use `snake_case` or hyphenated names such as `initial-country`.

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
- Set `"include_transit": false` to drop flight duration from the report.
- Set `"full_year": true` to force a complete year.
- Ambiguous or nonexistent local times during DST transitions are rejected so the
  input can be corrected explicitly.

## Run

```sh
cargo run -- --config examples/trip.json
```

In a normal terminal this opens the Ratatui dashboard. Press `q` or `Esc` to
quit.

For text output:

```sh
cargo run -- --config examples/trip.json --summary
```

If `full_year` is not set, the current year is counted until now and other years
are counted as a full calendar year.
