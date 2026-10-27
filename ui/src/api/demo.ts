import { addDays } from "../periods";
import { localDate } from "../format";
import type { Curve, Day, DaySummary, LiveEvent, Metric, MetricValue, Now, Registry, Reading, Stretches } from "./bindings";
import type { LiveHandlers, Transport } from "./transport";

const SLOT_SECONDS = 300;
const LIVE_INTERVAL_SECONDS = 5;
const OUTPUT_LIMIT_W = 3300;
const DERATED_LIMIT_W = 2400;
const HISTORY_DAYS = 365;
const CURVE_DAYS = 14;
const CLOUD_PASS_CACHE_LIMIT = 400;
const THERMAL_CYCLE_MINUTES = 20;
const PROFILE_CACHE_LIMIT = 45;
// Keep lifetime yield cumulative from a fixed, fictional installation date.
const LIFETIME_INSTALL_DATE = "2025-10-28";
const dailyYieldCache = new Map<string, number>();
const lifetimeBeforeCache = new Map<string, number>([[LIFETIME_INSTALL_DATE, 0]]);

/** Use the visitor's browser timezone throughout the static demo. */
function browserTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

const timezone = browserTimezone();

const metrics: Metric[] = [
  { key: "pv_volts", label: "Voltage", unit: "V", section: "solar", slots: { one: { index: 4, signed: false } }, scale: 0.1, decimals: 1, chart: { name: "voltage", hidden: false }, peak: "while_producing", feature: null, summary: null, note: null },
  { key: "pv_amps", label: "Current", unit: "A", section: "solar", slots: { one: { index: 8, signed: false } }, scale: 0.1, decimals: 1, chart: { name: "current", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "pv_power", label: "Power", unit: "W", section: "solar", slots: { one: { index: 13, signed: false } }, scale: 1, decimals: 0, chart: { name: "power", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "today_kwh", label: "Today", unit: "kWh", section: "solar", slots: { one: { index: 21, signed: false } }, scale: 0.1, decimals: 1, chart: { name: "power", hidden: true }, peak: null, feature: { label: "Generated today", subtle: false }, summary: "generated", note: null },
  { key: "total_kwh", label: "All time", unit: "kWh", section: "solar", slots: { two: { low: 19, high: 20, signed: false } }, scale: 0.1, decimals: 1, chart: null, peak: null, feature: null, summary: "lifetime", note: null },
  { key: "ac_volts", label: "Voltage", unit: "V", section: "grid", slots: { one: { index: 0, signed: false } }, scale: 0.1, decimals: 1, chart: { name: "voltage", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "ac_amps", label: "Current", unit: "A", section: "grid", slots: { one: { index: 1, signed: true } }, scale: 0.1, decimals: 1, chart: { name: "current", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "ac_power", label: "Power", unit: "W", section: "grid", slots: { one: { index: 3, signed: true } }, scale: 1, decimals: 0, chart: { name: "power", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "grid_frequency", label: "Frequency", unit: "Hz", section: "grid", slots: { one: { index: 2, signed: false } }, scale: 0.01, decimals: 2, chart: null, peak: null, feature: null, summary: null, note: null },
  { key: "inverter_temperature", label: "Temperature", unit: "C", section: "inverter", slots: { one: { index: 23, signed: false } }, scale: 1, decimals: 0, chart: { name: "temperature", hidden: false }, peak: "all_day", feature: null, summary: null, note: null },
  { key: "run_mode", label: "Run mode", unit: "", section: "inverter", slots: { one: { index: 10, signed: false } }, scale: 1, decimals: 0, chart: null, peak: null, feature: null, summary: null, note: null },
  { key: "rated_power", label: "Rated power", unit: "W", section: "inverter", slots: { one: { index: 22, signed: false } }, scale: 1, decimals: 0, chart: null, peak: null, feature: null, summary: null, note: "Constant on this unit" },
  { key: "secondary_temperature", label: "Second temperature", unit: "C", section: "inverter", slots: { one: { index: 24, signed: false } }, scale: 1, decimals: 0, chart: { name: "temperature", hidden: false }, peak: "all_day", feature: null, summary: null, note: "Illustrative secondary sensor" },
  { key: "grid_countdown", label: "Connecting to the grid in", unit: "s", section: "inverter", slots: { one: { index: 25, signed: false } }, scale: 1, decimals: 0, chart: null, peak: null, feature: null, summary: null, note: null },
  { key: "running_hours", label: "Running hours", unit: "h", section: "inverter", slots: { one: { index: 28, signed: false } }, scale: 0.1, decimals: 1, chart: null, peak: null, feature: null, summary: null, note: "Synthetic demo value" },
];

const registry: Registry = {
  roles: { hero: "pv_power", run_mode: "run_mode", daily_yield: "today_kwh", lifetime_yield: "total_kwh", grid_countdown: "grid_countdown", output: "ac_power" },
  metrics,
  site: {
    inverter_serial: "X.X.X",
    rating_kw: 3,
    max_output_kw: OUTPUT_LIMIT_W / 1000,
    timezone,
    poll_interval_seconds: LIVE_INTERVAL_SECONDS,
    co2_kg_per_kwh: 0.71,
    panel_kwp: 3.665,
  },
};

type Sample = {
  ts: number;
  values: Record<string, MetricValue | null>;
  power: number;
  atLimit: boolean;
  derating: boolean;
};

type DayProfile = {
  date: string;
  from: number;
  to: number;
  samples: Sample[];
  yieldKwh: number;
};

function round(value: number, digits = 1): number {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

function dateStart(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  return Math.floor(new Date(year, month - 1, day).getTime() / 1000);
}

function todayDate(): string {
  return localDate(Date.now() / 1000, timezone);
}

function localHour(ts: number): number {
  const date = new Date(ts * 1000);
  return date.getHours() + date.getMinutes() / 60 + date.getSeconds() / 3600;
}

function daylight(hour: number): number {
  if (hour <= 6 || hour >= 18) return 0;
  return Math.max(0, Math.sin(((hour - 6) / 12) * Math.PI));
}

/** Stable date hash: historical values remain identical between page loads. */
function hashDate(date: string): number {
  let hash = 2_166_136_261;
  for (let index = 0; index < date.length; index++) {
    hash ^= date.charCodeAt(index);
    hash = Math.imul(hash, 16_777_619);
  }
  return hash >>> 0;
}

function randomUnit(seed: number, channel: number): number {
  const value = Math.sin(seed * 0.000001 + channel * 78.233) * 43_758.5453;
  return value - Math.floor(value);
}

function dayOrdinal(date: string): number {
  return Math.floor(Date.parse(`${date}T00:00:00Z`) / 86_400_000);
}

/** A few hot days each month make thermal derating visible without dominating the history. */
function hasThermalEvent(date: string): boolean {
  return ((dayOrdinal(date) + 4) % 9) === 0;
}

type CloudPass = {
  centreHour: number;
  leadingWidthHours: number;
  trailingWidthHours: number;
  depth: number;
};

const cloudPassCache = new Map<string, CloudPass[]>();

/** Generate a repeatable schedule of moving cloud fronts for a local calendar day. */
function cloudPassesFor(date: string): CloudPass[] {
  const cached = cloudPassCache.get(date);
  if (cached) {
    cloudPassCache.delete(date);
    cloudPassCache.set(date, cached);
    return cached;
  }

  const seed = hashDate(date);
  const count = 3 + Math.floor(randomUnit(seed, 50) * 3);
  const passes = Array.from({ length: count }, (_, index): CloudPass => {
    const channel = 100 + index * 4;
    return {
      centreHour: 7.2 + randomUnit(seed, channel) * 9.6,
      leadingWidthHours: 0.14 + randomUnit(seed, channel + 1) * 0.28,
      trailingWidthHours: 0.18 + randomUnit(seed, channel + 2) * 0.42,
      depth: 0.14 + randomUnit(seed, channel + 3) * 0.34,
    };
  }).sort((left, right) => left.centreHour - right.centreHour);

  cloudPassCache.set(date, passes);
  while (cloudPassCache.size > CLOUD_PASS_CACHE_LIMIT) {
    const oldest = cloudPassCache.keys().next().value;
    if (oldest === undefined) break;
    cloudPassCache.delete(oldest);
  }
  return passes;
}

function cloudFactor(date: string, hour: number): number {
  const seed = hashDate(date);
  const dailySky = 0.91 + randomUnit(seed, 1) * 0.09;
  const waves = 0.975 + 0.018 * Math.sin(hour * 1.2 + randomUnit(seed, 2) * 6)
    + 0.012 * Math.cos(hour * 2.1 + randomUnit(seed, 3) * 6);

  // Each cloud front has a smooth, asymmetric profile as it crosses the sun.
  let clouds = 1;
  for (const pass of cloudPassesFor(date)) {
    const distance = hour - pass.centreHour;
    const width = distance < 0 ? pass.leadingWidthHours : pass.trailingWidthHours;
    const cover = Math.exp(-0.5 * (distance / width) ** 2);
    clouds *= 1 - pass.depth * cover;
  }

  // Cloud patterns change by date, but are identical across reloads and clients.
  return Math.max(0.52, Math.min(1, dailySky * waves * clouds));
}

function smoothStep(value: number): number {
  const t = Math.max(0, Math.min(1, value));
  return t * t * (3 - 2 * t);
}

/** Smoothly bring heat stress in and out around the hottest part of the day. */
function thermalEnvelope(hour: number): number {
  const rampUp = smoothStep((hour - 9.5) / 1.5);
  const rampDown = 1 - smoothStep((hour - 12.75) / 1.5);
  return rampUp * rampDown;
}

/** Stable cycle phase with a date-specific offset on the five-minute grid. */
function thermalPhase(date: string, hour: number): number {
  const seed = hashDate(date);
  const dailyShift = Math.floor(randomUnit(seed, 80) * 4) * 5;
  const minutes = hour * 60 + dailyShift;
  return ((minutes % THERMAL_CYCLE_MINUTES) + THERMAL_CYCLE_MINUTES) % THERMAL_CYCLE_MINUTES;
}

/** A deterministic triangle wave: a rise over 10 minutes, then a fall over 10. */
function thermalWave(date: string, hour: number, phaseShiftMinutes = 0): number {
  const phase = (thermalPhase(date, hour) + phaseShiftMinutes) % THERMAL_CYCLE_MINUTES;
  const triangle = 1 - Math.abs(phase / (THERMAL_CYCLE_MINUTES / 2) - 1);
  return triangle * 2 - 1;
}

type ThermalResult = {
  inverterTemperature: number;
  secondaryTemperature: number;
  derating: boolean;
};

function thermalAt(date: string, hour: number): ThermalResult {
  const seed = hashDate(date);
  const sun = daylight(hour);
  const normalPrimary = 28 + sun * (26 + randomUnit(seed, 41) * 4);
  const normalSecondary = 27 + sun * 18;
  const envelope = hasThermalEvent(date) ? thermalEnvelope(hour) : 0;

  // Heat stress moves toward ~81°C/~66°C, then oscillates rather than pinning
  // either sensor at its maximum. At five-minute sample intervals, the peak
  // should usually occupy only one sample before cooling again.
  const primaryBase = normalPrimary + (81 - normalPrimary) * envelope;
  const secondaryBase = normalSecondary + (66 - normalSecondary) * envelope;
  const primaryWave = thermalWave(date, hour);
  const secondaryWave = thermalWave(date, hour, 5);
  const inverterTemperature = Math.round(Math.max(25, Math.min(85, primaryBase + 4 * envelope * primaryWave)));
  const secondaryTemperature = Math.round(Math.max(25, Math.min(69, secondaryBase + 3 * envelope * secondaryWave)));

  // Apply the lower power ceiling only during the hot part of the cycle. This
  // gives a short, repeated derating interval with recovery between peaks.
  const phase = thermalPhase(date, hour);
  const derating = envelope >= 0.5
    && phase >= 5
    && phase < 15
    && inverterTemperature >= 80;
  return { inverterTemperature, secondaryTemperature, derating };
}

type PowerResult = {
  power: number;
  potential: number;
  atLimit: boolean;
  derating: boolean;
  sun: number;
  inverterTemperature: number;
  secondaryTemperature: number;
};

function powerAt(date: string, hour: number): PowerResult {
  const sun = daylight(hour);
  const potential = Math.max(0, Math.round(3_800 * sun ** 1.8 * cloudFactor(date, hour)));
  const thermal = thermalAt(date, hour);
  const cap = thermal.derating ? DERATED_LIMIT_W : OUTPUT_LIMIT_W;
  const power = Math.min(potential, cap);
  return {
    power,
    potential,
    atLimit: !thermal.derating && potential >= OUTPUT_LIMIT_W * 0.97,
    derating: thermal.derating,
    sun,
    inverterTemperature: thermal.inverterTemperature,
    secondaryTemperature: thermal.secondaryTemperature,
  };
}

/** Integrate a day's deterministic power profile into generated kWh. */
function dailyYieldFor(date: string): number {
  const cached = dailyYieldCache.get(date);
  if (cached !== undefined) return cached;

  const from = dateStart(date);
  const to = dateStart(addDays(date, 1));
  let yieldKwh = 0;
  for (let ts = from; ts < to; ts += SLOT_SECONDS) {
    yieldKwh += powerAt(date, localHour(ts)).power * SLOT_SECONDS / 3_600_000;
  }
  dailyYieldCache.set(date, yieldKwh);
  return yieldKwh;
}

/** Total energy generated before this date, memoised as a daily prefix sum. */
function lifetimeBefore(date: string): number {
  if (date <= LIFETIME_INSTALL_DATE) return 0;

  const cached = lifetimeBeforeCache.get(date);
  if (cached !== undefined) return cached;

  // Find the nearest earlier prefix, then fill the cache forward only once.
  let anchor = date;
  while (anchor > LIFETIME_INSTALL_DATE && !lifetimeBeforeCache.has(anchor)) {
    anchor = addDays(anchor, -1);
  }

  let total = lifetimeBeforeCache.get(anchor) ?? 0;
  let cursor = anchor;
  while (cursor < date) {
    total += dailyYieldFor(cursor);
    cursor = addDays(cursor, 1);
    lifetimeBeforeCache.set(cursor, total);
  }
  return total;
}

/** Lifetime yield is the completed synthetic days plus today's accumulated yield. */
function lifetimeAt(date: string, yieldKwh: number): number {
  return round(lifetimeBefore(date) + yieldKwh);
}

function valuesFor(date: string, powerResult: PowerResult, yieldKwh: number): Record<string, MetricValue | null> {
  const { power, sun, inverterTemperature, secondaryTemperature } = powerResult;
  const pvVolts = sun > 0 ? 190 + sun * 20 : 0;
  return {
    pv_volts: round(pvVolts),
    pv_amps: pvVolts > 0 ? round(power / pvVolts) : 0,
    pv_power: power,
    today_kwh: round(yieldKwh),
    total_kwh: lifetimeAt(date, yieldKwh),
    ac_volts: 236.1,
    ac_amps: round(power / 236.1),
    ac_power: power,
    grid_frequency: 50,
    inverter_temperature: inverterTemperature,
    run_mode: "Normal",
    rated_power: 3000,
    secondary_temperature: secondaryTemperature,
    grid_countdown: 0,
    running_hours: round(2_000 + Math.max(0, dayOrdinal(date) - dayOrdinal("2024-01-01")) * 12),
  };
}

function visibleLength(profile: DayProfile, date: string): number {
  if (date !== todayDate()) return profile.samples.length;
  const now = Math.floor(Date.now() / 1000);
  const count = Math.floor((now - profile.from) / SLOT_SECONDS) + 1;
  return Math.max(1, Math.min(profile.samples.length, count));
}

function peaksOf(samples: Sample[]): Record<string, number | null> {
  const peaks: Record<string, number | null> = {};
  for (const metric of metrics.filter((item) => item.peak)) {
    const values = samples
      .map((sample) => sample.values[metric.key])
      .filter((value): value is number => typeof value === "number");
    peaks[metric.key] = values.length ? Math.max(...values) : null;
  }
  return peaks;
}

function stretchesOf(samples: Sample[]): Stretches {
  const atLimit: Array<[number, number]> = [];
  const derating: Array<[number, number]> = [];
  let limitStart: number | null = null;
  let derateStart: number | null = null;
  let previous: Sample | null = null;

  for (const sample of samples) {
    if (sample.atLimit && limitStart === null) limitStart = sample.ts;
    if (!sample.atLimit && limitStart !== null) {
      atLimit.push([limitStart, sample.ts]);
      limitStart = null;
    }
    if (sample.derating && derateStart === null) derateStart = sample.ts;
    if (!sample.derating && derateStart !== null) {
      derating.push([derateStart, sample.ts]);
      derateStart = null;
    }
    previous = sample;
  }

  if (limitStart !== null && previous) atLimit.push([limitStart, previous.ts + SLOT_SECONDS]);
  if (derateStart !== null && previous) derating.push([derateStart, previous.ts + SLOT_SECONDS]);
  return { at_limit: atLimit, derating };
}

function summaryFor(date: string, samples: Sample[], currentValues?: Record<string, MetricValue | null | undefined>): DaySummary {
  if (samples.length === 0) {
    return {
      date, samples: 0, yield_kwh: 0, peak_power_w: 0, peak_at: null,
      producing_from: null, producing_until: null, lifetime_kwh: lifetimeAt(date, 0),
      fault_readings: 0, limit_seconds: 0, derate_count: 0, derate_seconds: 0,
    };
  }

  const powers = samples.map((sample) => ({ ts: sample.ts, power: Number(sample.values.pv_power ?? 0) }));
  if (currentValues) powers.push({ ts: Math.floor(Date.now() / 1000), power: Number(currentValues.pv_power ?? 0) });
  const peak = powers.reduce((best, item) => item.power > best.power ? item : best, powers[0]);
  const producing = powers.filter((item) => item.power > 0);
  const latest = currentValues ?? samples[samples.length - 1].values;
  const derateSamples = samples.filter((sample) => sample.derating).length;
  const limitSamples = samples.filter((sample) => sample.atLimit).length;
  const derateCount = stretchesOf(samples).derating.length;

  return {
    date,
    samples: samples.length,
    yield_kwh: Number(latest.today_kwh ?? 0),
    peak_power_w: peak.power,
    peak_at: peak.ts,
    producing_from: producing[0]?.ts ?? null,
    producing_until: producing.at(-1)?.ts ?? null,
    lifetime_kwh: Number(latest.total_kwh ?? lifetimeAt(date, 0)),
    fault_readings: 0,
    limit_seconds: limitSamples * SLOT_SECONDS,
    derate_count: derateCount,
    derate_seconds: derateSamples * SLOT_SECONDS,
  };
}

function readingAt(ts: number, sample: Sample, date: string): Reading {
  const elapsed = Math.max(0, Math.min(SLOT_SECONDS, ts - sample.ts));
  const yieldKwh = Number(sample.values.today_kwh ?? 0) + sample.power * elapsed / 3_600_000;
  const hour = localHour(ts);
  const result = powerAt(date, hour);
  const values = valuesFor(date, result, yieldKwh);
  return {
    ts,
    values,
    device: { ver: "x.x.x", inverter_type: 22 },
    health: { run_mode: "Normal", severity: "ok", fault: null },
  };
}

function emptyDay(date: string): Day {
  return {
    date,
    from: dateStart(date),
    to: dateStart(addDays(date, 1)),
    count: 0,
    bucket_seconds: null,
    ts: [],
    stretches: { at_limit: [], derating: [] },
    series: {},
    peaks: {},
    closing: {},
  };
}

export class DemoTransport implements Transport {
  private readonly profiles = new Map<string, DayProfile>();
  private readonly exportUrls = new Map<string, string>();

  registry(): Promise<Registry> {
    return Promise.resolve(registry);
  }

  private profile(date: string): DayProfile {
    const cached = this.profiles.get(date);
    if (cached) {
      this.profiles.delete(date);
      this.profiles.set(date, cached);
      return cached;
    }

    const from = dateStart(date);
    const to = dateStart(addDays(date, 1));
    const samples: Sample[] = [];
    let yieldKwh = 0;
    let previousPower = 0;
    for (let ts = from; ts < to; ts += SLOT_SECONDS) {
      const hour = localHour(ts);
      const result = powerAt(date, hour);
      if (samples.length > 0) yieldKwh += previousPower * SLOT_SECONDS / 3_600_000;
      const values = valuesFor(date, result, yieldKwh);
      samples.push({
        ts,
        values,
        power: result.power,
        atLimit: result.atLimit,
        derating: result.derating,
      });
      previousPower = result.power;
    }
    // Include the final interval in the full-day yield. The final sample is
    // after sunset, so this usually adds zero while keeping the calculation general.
    if (samples.length > 0) {
      yieldKwh += previousPower * SLOT_SECONDS / 3_600_000;
      const last = samples[samples.length - 1];
      last.values.today_kwh = round(yieldKwh);
      last.values.total_kwh = lifetimeAt(date, yieldKwh);
    }

    const profile = { date, from, to, samples, yieldKwh };
    this.profiles.set(date, profile);
    while (this.profiles.size > PROFILE_CACHE_LIMIT) {
      const oldest = this.profiles.keys().next().value;
      if (oldest === undefined) break;
      this.profiles.delete(oldest);
    }
    return profile;
  }

  private visibleSamples(date: string): Sample[] {
    const profile = this.profile(date);
    return profile.samples.slice(0, visibleLength(profile, date));
  }

  private makeDay(date: string): Day {
    const today = todayDate();
    const earliest = addDays(today, -(HISTORY_DAYS - 1));
    if (date < earliest || date > today) return emptyDay(date);

    const samples = this.visibleSamples(date);
    const series: Record<string, Array<number | null>> = {};
    for (const metric of metrics.filter((item) => item.chart)) {
      series[metric.key] = samples.map((sample) => {
        const value = sample.values[metric.key];
        return typeof value === "number" ? value : null;
      });
    }
    const closing = samples[samples.length - 1]?.values ?? {};
    return {
      date,
      from: dateStart(date),
      to: dateStart(addDays(date, 1)),
      count: samples.length,
      bucket_seconds: null,
      ts: samples.map((sample) => sample.ts),
      stretches: stretchesOf(samples),
      series,
      peaks: peaksOf(samples),
      closing,
    };
  }

  private makeSummary(date: string): DaySummary {
    const today = todayDate();
    const earliest = addDays(today, -(HISTORY_DAYS - 1));
    if (date < earliest || date > today) {
      return {
        date, samples: 0, yield_kwh: null, peak_power_w: null, peak_at: null,
        producing_from: null, producing_until: null, lifetime_kwh: null,
        fault_readings: 0, limit_seconds: null, derate_count: null, derate_seconds: null,
      };
    }
    const samples = this.visibleSamples(date);
    if (date === today) {
      const now = Math.floor(Date.now() / 1000);
      const slotIndex = Math.max(0, Math.min(samples.length - 1, Math.floor((now - dateStart(date)) / SLOT_SECONDS)));
      const reading = readingAt(now, samples[slotIndex], date);
      return summaryFor(date, samples, reading.values);
    }
    return summaryFor(date, samples);
  }

  now(): Promise<Now> {
    const nowSeconds = Math.floor(Date.now() / 1000);
    const date = localDate(nowSeconds, timezone);
    const profile = this.profile(date);
    const samples = this.visibleSamples(date);
    const slotIndex = Math.max(0, Math.min(samples.length - 1, Math.floor((nowSeconds - profile.from) / SLOT_SECONDS)));
    const latestReading = readingAt(nowSeconds, samples[slotIndex], date);
    const today = summaryFor(date, samples, latestReading.values);
    const peaks = peaksOf(samples);
    for (const [key, value] of Object.entries(latestReading.values)) {
      const metric = metrics.find((item) => item.key === key);
      if (metric?.peak && typeof value === "number") peaks[key] = Math.max(peaks[key] ?? value, value);
    }

    const completedDays = Array.from({ length: CURVE_DAYS }, (_, index) => addDays(date, -(index + 1)))
      .map((day) => this.profile(day).yieldKwh);
    const average = completedDays.reduce((sum, value) => sum + value, 0) / completedDays.length;
    const localHourNow = localHour(nowSeconds);
    const projection: Now["projection"] = localHourNow < 7
      ? { state: "too_early", from: "07:00" }
      : localHourNow >= 18
        ? { state: "finished", yield_kwh: Number(latestReading.values.today_kwh ?? 0) }
        : {
            state: "on_pace",
            expected_kwh: round(average),
            low_kwh: round(average * 0.85),
            high_kwh: round(average * 1.15),
            based_on_days: completedDays.length,
          };

    const earliest = addDays(date, -(HISTORY_DAYS - 1));
    let stored = samples.length;
    for (let offset = 1; offset < HISTORY_DAYS; offset++) {
      const historicalDate = addDays(date, -offset);
      stored += (dateStart(addDays(historicalDate, 1)) - dateStart(historicalDate)) / SLOT_SECONDS;
    }
    return Promise.resolve({
      reading: latestReading,
      peaks,
      today,
      stretches: stretchesOf(samples),
      poller: { sleeping: false, resumes_at: null, window: { from: "06:00", until: "18:00" } },
      projection,
      stored,
      earliest: dateStart(earliest),
      storage_bytes: 4_194_304,
    });
  }

  curve(): Promise<Curve> {
    const date = todayDate();
    const profiles = Array.from({ length: CURVE_DAYS }, (_, index) => this.profile(addDays(date, -(index + 1))));
    const length = 18 * 3600 / SLOT_SECONDS + 1;
    const curves = profiles.map((profile) => {
      const buckets: number[][] = Array.from({ length }, () => []);
      for (const sample of profile.samples) {
        const time = new Date(sample.ts * 1000);
        const localSeconds = time.getHours() * 3600 + time.getMinutes() * 60;
        const index = Math.round(localSeconds / SLOT_SECONDS);
        if (index >= 0 && index < length) buckets[index].push(sample.power);
      }
      const powers = buckets.map((values) => values.length
        ? values.reduce((sum, value) => sum + value, 0) / values.length
        : 0);
      const cumulative = [0];
      for (let index = 1; index < powers.length; index++) {
        cumulative.push(cumulative[index - 1] + ((powers[index - 1] + powers[index]) / 2) * SLOT_SECONDS);
      }
      const total = cumulative[cumulative.length - 1] || 1;
      return cumulative.map((value) => value / total);
    });

    const quantile = (values: number[], fraction: number): number => {
      const sorted = [...values].sort((a, b) => a - b);
      return sorted[Math.floor((sorted.length - 1) * fraction)] ?? 0;
    };
    const atBoundary = (fraction: number) => Array.from({ length }, (_, index) =>
      quantile(curves.map((curve) => curve[index]), fraction));

    return Promise.resolve({
      date,
      slot_seconds: SLOT_SECONDS,
      based_on_days: profiles.length,
      median: atBoundary(0.5),
      lower: atBoundary(0.25),
      upper: atBoundary(0.75),
    });
  }

  day(date: string, _points?: number): Promise<Day> {
    return Promise.resolve(this.makeDay(date));
  }

  days(from: string, to: string): Promise<DaySummary[]> {
    const today = todayDate();
    const earliest = addDays(today, -(HISTORY_DAYS - 1));
    const start = from < earliest ? earliest : from;
    const end = to > today ? today : to;
    const rows: DaySummary[] = [];
    for (let date = start; date <= end && rows.length < HISTORY_DAYS; date = addDays(date, 1)) {
      rows.push(this.makeSummary(date));
    }
    return Promise.resolve(rows);
  }

  /** Emit fresh synthetic telemetry; this transport does not open a network connection. */
  openLive(handlers: LiveHandlers): () => void {
    let closed = false;
    handlers.opened();
    const emit = () => {
      if (closed) return;
      void this.now().then((now) => {
        if (!closed) {
          const event: LiveEvent = { type: "reading", ...now };
          handlers.event(event);
        }
      }).catch(() => handlers.failed(false));
    };
    emit();
    const timer = setInterval(emit, LIVE_INTERVAL_SECONDS * 1_000);
    return () => {
      closed = true;
      clearInterval(timer);
    };
  }

  /** Build a CSV entirely in the browser, with no request to the server. */
  exportUrl(date: string, refresh = false): string {
    const cached = this.exportUrls.get(date);
    if (cached && !(refresh && date === todayDate())) return cached;
    if (cached) URL.revokeObjectURL(cached);
    const day = this.makeDay(date);
    const keys = Object.keys(day.series);
    const rows = [["timestamp", ...keys].join(",")];
    day.ts.forEach((ts, index) => {
      rows.push([ts, ...keys.map((key) => day.series[key]?.[index] ?? "")].join(","));
    });
    const url = URL.createObjectURL(new Blob([rows.join("\n")], { type: "text/csv;charset=utf-8" }));
    this.exportUrls.set(date, url);
    return url;
  }
}
