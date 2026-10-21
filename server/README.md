# SolaX Monitor Server

The Rust server for a self-hosted SolaX inverter monitor. It polls the inverter's local API, stores raw readings and historical data in SQLite, and serves the web dashboard and `/api/v1` endpoints over your local network.

Tested with a **SolaX X1 Mini G4 configured for a maximum output of 3.3 kW**. Other inverter models and configurations have not been verified.

The server is designed to work locally without relying on the SolaX cloud. Its metric registry drives how raw inverter registers are interpreted, stored, and exposed to the dashboard.

## Adding a metric

Every metric is declared in `metrics.toml` and nowhere else. Add a block under
its section and restart:

```toml
[[inverter]]
key = "control_temperature"
label = "Control board temperature"
unit = "C"
index = 24
```

On boot the app adds a virtual generated column over the raw array that is
already stored for every sample. That costs no table rewrite, and the new metric
has correct values for history going back to the first row. It then appears in
`/api/v1/registry`, and the screens render it because they loop over
descriptors rather than naming metrics. No SQL, no Rust and no HTML edit.

Change an existing block's `index` and the column is rebuilt to match, so the
TOML stays authoritative.

The section is the list a block sits in: `[[solar]]`, `[[grid]]` or
`[[inverter]]`. Sections reach the page in that order, and metrics keep the
order they are written in within each. A misspelled section, or any key the
registry does not know, stops the app at boot naming what it did not
recognise, rather than being quietly ignored.

`index` is the metric's slot in the Data array. Each slot holds a whole number
from 0 to 65,535, which inverter documentation calls a register, and most
readings fit in one: `index = 4`. A number that outgrows one slot is split
across two like an odometer with two wheels. The low slot counts up to 65,535
and wraps to zero, the high slot counts the wraps, and the value is
low + 65,536 × high. The lifetime total is written that way, low slot first:
`index = [19, 20]`. In September 2026 those slots read 18,717 and 0, which is
1,871.7 kWh; the high slot first ticks over at 6,553.6 kWh.

`signed = true` marks a number that can go below zero, such as grid current
and power. A slot has no room for a minus sign, so the top half of its range
stands for negatives: 65,535 reads as -1.

Two metrics play a role of their own, and the file names each once at the top:

- `hero` is the big number on the page, and its line tells the page when the
  plant is producing. It must have a chart, and that chart is the main one.
- `run_mode` holds the inverter's run mode, which raises the fault badge. A
  name that matches no metric stops the app at boot, so renaming the metric
  cannot quietly switch the badge off.

The screens receive each metric from `/api/v1/registry` in exactly the shape
the server holds it, so a field reads the same in Rust and in the browser.

### Scale and decimals

The inverter sends every register as a whole number, so a reading with a
fraction travels as a count of smaller units. `scale` is the size of one count
in the metric's `unit`, and the page shows the register multiplied by it. Grid
voltage arrives as `2222` and a scale of `0.1` shows it as 222.2 V. Frequency
arrives as `4959` and `0.01` makes it 49.59 Hz. A register that already counts
whole watts keeps the default of `1.0`. For a pair, the scale applies to the
combined 32 bit value.

Because the register is whole, `scale` is also the smallest step the value can
take. Grid voltage moves from 222.2 to 222.3 and never shows 222.25, and the
day's yield climbs in 0.1 kWh steps, which is the staircase described under
Charts.

`decimals` is only how many digits the page shows after the point, and values
are rounded to it. The rounding keeps floating point noise off the page:
`2321 × 0.1` is `232.10000000000002` in binary arithmetic. Digits beyond what
the scale carries add only zeros, so a register counting 0.1 kWh shown at two
decimals reads 17.70 and can never read 17.73.

The database keeps the raw whole number, and scaling happens only when a value
is sent to the page. A day's peak is the highest raw register, which is also
the highest scaled value only while the scale is positive, so the app refuses
to start with a scale of zero or below.

### Peaks, summaries and the hero card

Two fields shape what a metric's figures mean:

- `peak` shows today's high beside the live value and marks it on the chart.
  `"all_day"` counts every reading. `"while_producing"` counts only samples
  where solar power is above zero. PV voltage uses it, because with nothing
  drawing current the panels sit at open circuit, and that reads higher than
  any voltage seen while generating. Grid voltage deliberately does not: it
  reads zero while disconnected, and a high grid voltage while idle is real and
  is exactly what trips the inverter. Counting while producing also shapes the
  chart's value axis, at a different threshold, as described under Charts.
- `summary = "..."` states the metric's closing value in a past day's chart
  caption, worded by it. The day's yield carries `generated` and the lifetime
  total carries `lifetime`. A charted metric with one is also named by it in
  the legend, which is why the yield line reads "Generated" on any day rather
  than "Generated today".

`feature = "..."` lifts a metric onto the hero card under the meter, worded by
the string, in place of its row in its section. Written as
`feature = { label = "...", subtle = true }` it renders quietly, for context
beside the headline figure. `note` adds small print under a metric's label.

## Charts

`chart = "name"` puts a metric on the chart of that name, and metrics naming
the same chart share it. The chart holding the hero metric is the main one,
titled by its name. Every other chart is a tab in the slot beneath it, in the
order its first metric appears, reading the sections solar, grid, inverter. A
last tab, Off, removes the slot and gives its height to the main chart, and
choosing any chart brings it back. The choice is remembered per device. That
button is why `off` is refused as a chart name at boot. The hero metric must
have a chart too, since the page reads its line to tell when the plant is
producing.

Today that makes four. Power carries solar and grid power with the day's
running yield; voltage, current and temperature each pair the solar and grid
reading, or the two temperature sensors. The two charts share their time
axis: hovering one reads the same moment off the other, and dragging across
either zooms both. A zoom survives live readings, switching tabs and a
refetch after an outage, and a double click resets it.

The charts open on the part of the day worth reading: from the first reading
that produced power to the latest one, or on a past day to the last one that
produced. The minutes before generation starts, and on a past day those after
it ends, are left out: the inverter is waiting or checking then and produces
nothing. A double click returns to this window, so those readings stay out of
view. A page left open overnight moves its start to where generation began as
soon as it does.

A chart holds at most two units, one per axis. The first unit declared reads
off the left axis and a second off the right, and boot refuses a chart with a
third. The running yield is that second unit on the power chart. Its register
counts in 0.1 kWh steps, so the line climbs as a staircase, and its last point
is always the figure on the hero card. Its chart is written
`{ name = "power", hidden = true }`, so it starts switched off. Every legend entry switches its line on and off, and a
line that is off keeps its entry, dimmed, until it is brought back.

Colour follows the section, so solar is yellow and grid teal wherever they
appear. A second series from the same section on one chart, such as the yield
beside solar power, takes lilac instead.

Every metric with a `peak` gets its high point marked on its chart, under the
same rule as the card, so `"while_producing"` keeps the PV voltage marker off
the open circuit reading at dawn. The marker reads the window in view, so a
metric counted all day, such as grid voltage, can mark a lower figure than its
card when its high came before generation started. The labels sit in a strip
above the plot, joined to their dots by a hairline, so they never cover the
curve. On a narrow screen a row too wide for the plot spreads across the whole
chart, then drops the times, then keeps only the labels that fit.

Counting a peak `"while_producing"` also shapes the axis. At start-up the
inverter hunts for its operating point on a few watts, and PV voltage sits near
40 V for minutes before settling in the 205 to 255 V band. Fitting the axis to
that would squash the operating band into a thin line. So for such a metric,
the bottom of the axis counts only readings taken once the plant makes 2% of
its rating, while the top reaches every producing reading, the same rule as
the peak marker, so the marker always lands on the plot. The start-up
readings are still drawn, running off the bottom edge. Until more than one
reading in view passes start-up, as in the first minutes of the morning, the
line is fitted as it is. A zoom fits everything in view, since zooming in there
is asking to see it.

## Polling hours

```toml
poll_from = "05:00"
poll_until = "19:00"
```

Local times in the configured timezone. After dark the inverter powers down
and takes the dongle with it, so polling would only produce a failed request
and a warning line every minute until sunrise. Outside the window the poller
sleeps on a timer rather than waking each interval to find the same thing.

Each sleep is capped at fifteen minutes. Tokio's timers are monotonic and do
not advance while a machine is suspended, so a single long await could wake
well after sunrise; waking periodically bounds that at fifteen minutes and
costs about forty timer fires a night.

Leave both out to poll around the clock. Written the other way round, for
example `"22:00"` to `"06:00"`, the window wraps midnight.

The poller pushes its state over the live stream when it sleeps and wakes,
and again to each page as it connects, so a quiet night shows `Asleep until
05:00` rather than the fault styling. A page also asks `/api/v1/now`, which
carries the poller state, once when it loads and once each time it comes back
into view, because a stream that stayed open through hours of silence has
nothing new to announce. Neither is a timer: nothing asks on a schedule.

Only a reading pushed live counts as proof the poller is awake. The last stored
reading, fetched when the page opens or returns, says nothing about whether the
poller is running now, so it never clears the asleep state.

The browser reconnects a dropped stream by itself only some of the time. When
an open stream fails with an error it does not count as transient, it closes
the stream for good; Firefox logs that moment as "The connection to ... was
interrupted while the page was loading", whatever the page was doing. A page
left in view would then sit on Reconnecting until refreshed, so it retries on
its own, after five seconds and then backing off to once a minute. Once the
stream is back it catches up from the store, refetching the day if readings
were missed, so the curve has no hole where the outage was.

## Give the dongle a fixed address

Pin `192.168.29.10` to the dongle's MAC, `XX:XX:XX:XX:XX:XX`, as a DHCP
reservation in the router.

The dongle powers down with the inverter every evening and asks for a lease
again each morning, so without a reservation its address can move and
`config.toml` silently becomes wrong. This cannot be recovered automatically
from the ARP table: entries age out within minutes of a device going quiet, so
by sunrise there is nothing left to look the MAC up against. Finding it again
would mean sweeping the subnet to repopulate the table first. The reservation
costs nothing and removes the problem instead of detecting it.

## Running it

```sh
cp config.toml.example config.toml
chmod 600 config.toml
# put a real token in it: openssl rand -hex 16
docker compose up --build
```

Then open `http://<host>:8080/?token=YOUR_TOKEN` once per device. The token
moves into a cookie and the URL cleans itself up.

Without Docker: `cargo run`, with `config.toml` in the working directory.

## Endpoints

| Route | What it gives you |
|---|---|
| `/` | the screens, built from `ui/` and embedded in the binary |
| `/assets/...` | their scripts and styles |
| `/api/v1/...` | the API, described under API v1 below |
| `/healthz` | the one route with no token |

## Notes on this inverter

Confirmed against the device: type 22, dongle `SRXXXXXXX`, inverter
`XMXXXXXXX`, 103 values in `Data`.

Index meanings come from the type `18,22` table in
[solax-local-api-docs](https://github.com/nazar-pc/solax-local-api-docs), with
three additions found by diffing live samples:

- `Data[23]` is the inverter temperature. It is not in the published table.
- `Data[22]` holds a constant 3000, matching `Information[0] = 3.000`, so it is
  the nameplate rating in watts.
- `Data[28]` is a slow counter, not a constant. What it counts is not known yet.

Peak grid power delivered is `MAX(Data[3])`. The published `feedInPower` at
indices 72 and 73 sits in the all-zero tail because this unit has no CT meter,
so a tile driven from it would read zero forever.

`Information[1]` is the inverter type, and the type decides the whole index
layout. Every row stores `type` alongside the raw array, so a firmware change
that reshuffles indices shows up in the data instead of quietly corrupting it.

That `type` is the same enumeration the SolaX cloud API calls `deviceModel`:
22 is the X1-Mini-G4 and 18 is the X1-Boost-G4. It is why the reverse
engineered docs group `18,22` under one layout, and it is independent
confirmation that the index map used here belongs to this inverter.

## Storage

One row per poll in `samples`, holding the full `Data` array as JSON plus `sn`,
`ver` and `type`. `ts` is epoch seconds; days are resolved in the configured
timezone, because a UTC day on `Asia/Kolkata` would start at 05:30 local and cut
the solar curve in the middle of the morning.

Older rows imported from the Tk app's CSV files carry `source = 'csv'` and a
rebuilt array with JSON null at every index the CSV could not speak to, so an
unknown metric reads NULL rather than a convincing zero. The import itself is
gone; those rows stay as history.

## Faults

A faulted reply carries an `Error` object alongside `Data`:

```json
"Error": { "ErrorCode": "1,2,3", "ErrorData": [251.3, 0.0, 0, 99.3, ...] }
```

It sits outside `Data`, so no register and no metric entry can describe it. It
is stored verbatim in a `raw_error` column and logged as a warning, so a fault
that clears before anyone looks is still on record.

On the page it surfaces as a pill beside the status pill, red for a faulted
run mode and amber otherwise, listing the codes inline. Below 700px the
descriptions drop and the state alone remains. The alert does not depend on
the run mode alone: a reply can carry an `Error` while `Data[10]` still reads
Normal, and that still raises the pill.

`ErrorData` holds the conditions at the moment of the trip, which is usually
more telling than the registers in the same reply, since those describe the
state afterwards with the inverter already disconnected. It is kept in the
database rather than shown.

The integers in `ErrorCode` are not the IE numbers printed in the manual. The
names below come from what this inverter itself reported during a real fault:

| Code | Meaning |
|---|---|
| 1 | Grid Lost Fault |
| 2 | Grid Volt Fault |
| 3 | Grid Frequency Fault |

All three are grid faults, which is consistent with them appearing together:
losing the grid trips voltage and frequency protection at the same moment.

Anything else is shown as a bare code. A wrong fault name is worse than an
honest number when the point is to diagnose one.

The CSV export carries two extra columns, `error_codes` and `error_names`,
empty on a healthy reading. Both are present because they age differently: the
numbers are what the inverter actually sent and stay true whatever this
project later decides to call them, while the names are what a reader can act
on today.

The SolaX cloud Developer Portal does not settle this. Its alarm codes are a
different number space entirely, three digit values such as 60, 147 and 152,
carried with their own name and cause strings. Its `deviceStatus` values are
likewise cloud side, 100 to 177, rather than the 0 to 7 run mode the local API
returns. Reaching for the cloud to name a local fault would also give up the
one property that makes this worth running: it answers with no internet, no
account and no third party between you and your own inverter.

The run mode at `Data[10]` decides whether the badge appears at all: 3 and 4
are faults, 7 is EPS and shows as a warning, everything else is quiet. The
metric is the one `metrics.toml` names as `run_mode`, and a name that matches
no metric stops the app at boot, so renaming it cannot silently disable the
badge.

## A note on aggregate queries

SQLite turns a lone aggregate into an index seek, but asking for two in one
statement makes it scan the table instead: a single scan can only serve one
index. `Store::summary` therefore issues `COUNT(*)` and `MIN(ts)` as separate
statements. Measured on 300,000 rows of real shape:

| Query | Time | Plan |
|---|---|---|
| `COUNT(*), MIN(ts)` together | 10.5 ms | scan |
| `COUNT(*)` alone | 0.47 ms | covering index |
| `MIN(ts)` alone | 0.00 ms | rowid seek |

Combining them again would look tidier and cost twenty times more, growing
with the database. It is worth leaving apart.

Everything else is bounded to a single day. A day is a contiguous `ts` range,
so the peaks are a rowid scan over roughly 8,640 rows, about 3.6 ms with seven
peak metrics, and that does not change however long the database runs. The
indexes on the peak columns are not used by it and exist for a whole-history
query that nothing asks for yet.

## API v1

Everything a client needs, grouped by screen. Every response is a type from
the `solax-api` crate, and the TypeScript for each lives in
`crates/solax-api/bindings/`. Within v1, fields and routes are only added,
never renamed or removed.

| Route | Returns |
|---|---|
| `/api/v1/registry` | Roles, metrics, and site facts: serial, rating, timezone, poll interval, CO2 factor |
| `/api/v1/now` | The latest reading, today's peaks, today's day row, the poller state, the projection |
| `/api/v1/live` | Server-sent events: a `Now` on every reading, the poller state when it changes |
| `/api/v1/day?date=&points=` | One day's series on a shared time axis; `points` downsamples, keeping every peak exact |
| `/api/v1/days?from=&to=` | One row per day with readings, at most 400 days per request |
| `/api/v1/curve` | Today's typical-day shares (median, 25th and 75th percentile at each 5 minute boundary), for drawing the projection |
| `/api/v1/export?date=` | CSV regenerated from the database, faults included; `from` and `to` as epoch seconds override `date` |

Days are summarised once into the `daily` table when first asked for, as raw
counts, so changing a metric's scale or decimals never makes them stale.
Changing the timezone or a role metric's slots recomputes them.

`metrics.toml` names two more roles beside `hero` and `run_mode`:
`daily_yield`, the inverter's count of today's yield, and `lifetime_yield`.
`config.toml` takes an optional `[site]` table whose `co2_kg_per_kwh`
(default 0.71) turns yield into CO2 avoided.

The projection learns a typical day from the 14 most recent complete days in
the last 28, and says `not_enough_history` until it has 3.
