import type { Day, MetricValue, Stretches } from "../api/bindings";

type Column = Array<number | null>;

/** The longest step between neighbouring readings that still draws as one
 * line: three polls, at least a minute, and twice a downsampling bucket.
 * A day recorded less often than the poll rate, such as one read every five
 * minutes, counts three of its own usual steps instead. */
export function gapThreshold(intervalSeconds: number, bucketSeconds: number | null, usualStepSeconds = 0): number {
  return Math.max(60, intervalSeconds * 3, bucketSeconds ? bucketSeconds * 2 : 0, usualStepSeconds * 3);
}

/** The median step between neighbouring readings, or 0 with fewer than two. */
function usualStep(timestamps: number[]): number {
  const steps = timestamps.slice(1).map((timestamp, index) => timestamp - timestamps[index]);
  if (!steps.length) return 0;
  steps.sort((first, second) => first - second);
  return steps[Math.floor(steps.length / 2)];
}

/** Splices a null after each step longer than the threshold, so the line
 * breaks there instead of joining across time when nothing was recorded. */
export function insertGaps(timestamps: number[], columns: Column[], threshold: number): [number[], Column[]] {
  const outTimestamps: number[] = [];
  const outColumns: Column[] = columns.map(() => []);
  timestamps.forEach((timestamp, index) => {
    if (index > 0 && timestamp - timestamps[index - 1] > threshold) {
      outTimestamps.push(timestamps[index - 1] + 1);
      for (const column of outColumns) column.push(null);
    }
    outTimestamps.push(timestamp);
    columns.forEach((column, position) => outColumns[position].push(column[index] ?? null));
  });
  return [outTimestamps, outColumns];
}

/** One local day's charted readings, which live readings extend. */
export class DayData {
  private stretchList: Stretches;

  private constructor(
    readonly date: string,
    readonly from: number,
    readonly count: number,
    /** Every metric's value at the day's last reading. */
    readonly closing: Partial<Record<string, MetricValue | null>>,
    private readonly threshold: number,
    private readonly timestampList: number[],
    private readonly columns: Map<string, Column>,
    stretches: Stretches,
  ) {
    this.stretchList = stretches;
  }

  static fromDay(day: Day, chartKeys: string[], intervalSeconds: number): DayData {
    const threshold = gapThreshold(intervalSeconds, day.bucket_seconds, usualStep(day.ts));
    const [timestamps, columns] = insertGaps(day.ts, chartKeys.map((key) => day.series[key] ?? []), threshold);
    const byKey = new Map(chartKeys.map((key, position) => [key, columns[position]]));
    // An older server sends no stretches.
    const stretches = day.stretches ?? { at_limit: [], derating: [] };
    return new DayData(day.date, day.from, day.count, day.closing, threshold, timestamps, byKey, stretches);
  }

  /** The stretches to mark on the power chart: at the limit, and derated
   * for heat. */
  get stretches(): Stretches {
    return this.stretchList;
  }

  /** Today's stretches as of the latest reading, which replace the last set. */
  setStretches(stretches: Stretches): void {
    this.stretchList = stretches;
  }

  get timestamps(): number[] {
    return this.timestampList;
  }

  column(key: string): Column {
    return this.columns.get(key) ?? [];
  }

  latest(): number | null {
    return this.timestampList.length ? this.timestampList[this.timestampList.length - 1] : null;
  }

  /** Adds a live reading. A reading at or before the latest one is ignored,
   * so catching up after an outage never draws a reading twice. */
  append(ts: number, values: Record<string, number | null>): boolean {
    const last = this.latest();
    if (last !== null && ts <= last) return false;
    if (last !== null && ts - last > this.threshold) {
      this.timestampList.push(last + 1);
      for (const column of this.columns.values()) column.push(null);
    }
    this.timestampList.push(ts);
    for (const [key, column] of this.columns) column.push(values[key] ?? null);
    return true;
  }
}
