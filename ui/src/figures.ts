import type { DaySummary } from "./api/bindings";

/** What specific yield divides by: the panels' peak power when it is
 * configured, the inverter's rating otherwise. */
export interface Capacity { kw: number; of: "panels" | "inverter" }

export interface DayFigure { date: string; kwh: number }
export interface PeakFigure { date: string; watts: number; at: number | null }

/** Records over any run of days, a month's or a year's. */
export interface DayRecords {
  /** The best and lowest completed day; today's is still growing. */
  best: DayFigure | null;
  lowest: DayFigure | null;
  peak: PeakFigure | null;
  /** kWh per kW of capacity, and which capacity. */
  specificYield: { kwhPerKw: number; per: Capacity["of"] } | null;
  /** Seconds spent at the inverter's limit, or null when none is known. */
  limitSeconds: number | null;
  mostAtLimit: { date: string; seconds: number } | null;
  /** Times the inverter stepped down for heat, or null when the metrics
   * that show it are not named. */
  derateCount: number | null;
  derateSeconds: number | null;
  mostDerated: { date: string; count: number } | null;
}

export interface MonthFigures extends DayRecords {
  totalKwh: number;
  daysRecorded: number;
  averageKwh: number | null;
  /** Average per day against last month's, as a percentage. */
  comparedWithPrevious: number | null;
  co2Kg: number;
  faultDays: string[];
}

export interface MonthTotal { month: string; totalKwh: number; daysRecorded: number }

export interface YearFigures extends DayRecords {
  totalKwh: number;
  months: MonthTotal[];
  bestMonth: MonthTotal | null;
  co2Kg: number;
  lifetimeKwh: number | null;
  lifetimeCo2Kg: number | null;
}

function recorded(rows: DaySummary[]): DaySummary[] {
  return rows.filter((row) => row.samples > 0);
}

function totalOf(rows: DaySummary[]): number {
  return rows.reduce((sum, row) => sum + (row.yield_kwh ?? 0), 0);
}

function averageOf(rows: DaySummary[]): number | null {
  const days = recorded(rows);
  return days.length ? totalOf(days) / days.length : null;
}

/** The records over days that have readings, which yield totalKwh. */
function dayRecords(days: DaySummary[], totalKwh: number, today: string, capacity: Capacity): DayRecords {
  const completed = days
    .filter((row) => row.date < today && row.yield_kwh !== null)
    .map((row) => ({ date: row.date, kwh: row.yield_kwh as number }))
    .sort((first, second) => first.kwh - second.kwh || first.date.localeCompare(second.date));
  let peak: PeakFigure | null = null;
  for (const row of days) {
    if (row.peak_power_w !== null && (peak === null || row.peak_power_w > peak.watts)) {
      peak = { date: row.date, watts: row.peak_power_w, at: row.peak_at };
    }
  }
  let limitSeconds: number | null = null;
  let mostAtLimit: { date: string; seconds: number } | null = null;
  for (const row of days) {
    // A server that predates the figure leaves it out, so undefined too.
    if (row.limit_seconds == null) continue;
    limitSeconds = (limitSeconds ?? 0) + row.limit_seconds;
    if (mostAtLimit === null || row.limit_seconds > mostAtLimit.seconds) mostAtLimit = { date: row.date, seconds: row.limit_seconds };
  }
  let derateCount: number | null = null;
  let derateSeconds: number | null = null;
  let mostDerated: { date: string; count: number } | null = null;
  for (const row of days) {
    if (row.derate_count == null) continue;
    derateCount = (derateCount ?? 0) + row.derate_count;
    derateSeconds = (derateSeconds ?? 0) + (row.derate_seconds ?? 0);
    if (row.derate_count > 0 && (mostDerated === null || row.derate_count > mostDerated.count)) mostDerated = { date: row.date, count: row.derate_count };
  }
  return {
    best: completed.length ? completed[completed.length - 1] : null,
    lowest: completed.length ? completed[0] : null,
    peak,
    specificYield: capacity.kw > 0 ? { kwhPerKw: totalKwh / capacity.kw, per: capacity.of } : null,
    limitSeconds,
    mostAtLimit,
    derateCount,
    derateSeconds,
    mostDerated,
  };
}

export function monthFigures(rows: DaySummary[], previousRows: DaySummary[], today: string, capacity: Capacity, co2KgPerKwh: number): MonthFigures {
  const days = recorded(rows);
  const totalKwh = totalOf(days);
  const averageKwh = averageOf(days);
  const previousAverage = averageOf(previousRows);
  return {
    ...dayRecords(days, totalKwh, today, capacity),
    totalKwh,
    daysRecorded: days.length,
    averageKwh,
    comparedWithPrevious:
      averageKwh !== null && previousAverage !== null && previousAverage > 0 ? ((averageKwh - previousAverage) / previousAverage) * 100 : null,
    co2Kg: totalKwh * co2KgPerKwh,
    faultDays: days.filter((row) => row.fault_readings > 0).map((row) => row.date),
  };
}

export function yearFigures(
  rows: DaySummary[],
  year: number,
  today: string,
  capacity: Capacity,
  co2KgPerKwh: number,
  lifetimeKwh: number | null,
): YearFigures {
  const months: MonthTotal[] = Array.from({ length: 12 }, (_, index) => {
    const month = `${year}-${String(index + 1).padStart(2, "0")}`;
    const days = recorded(rows.filter((row) => row.date.startsWith(month)));
    return { month, totalKwh: Math.round(totalOf(days) * 1000) / 1000, daysRecorded: days.length };
  });
  const days = recorded(rows.filter((row) => row.date.startsWith(String(year))));
  const totalKwh = totalOf(days);
  const withData = months.filter((month) => month.daysRecorded > 0);
  return {
    ...dayRecords(days, totalKwh, today, capacity),
    totalKwh,
    months,
    bestMonth: withData.reduce<MonthTotal | null>((best, month) => (best === null || month.totalKwh > best.totalKwh ? month : best), null),
    co2Kg: totalKwh * co2KgPerKwh,
    lifetimeKwh,
    lifetimeCo2Kg: lifetimeKwh === null ? null : lifetimeKwh * co2KgPerKwh,
  };
}
