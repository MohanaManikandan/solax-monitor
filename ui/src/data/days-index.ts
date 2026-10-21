import type { DaySummary } from "../api/bindings";
import { splitRange } from "../periods";

type FetchDays = (from: string, to: string) => Promise<DaySummary[]>;

/** Every local day that holds readings, as day rows: the day arrows step
 * through it, and Month and Year read from it. */
export class DaysIndex {
  private readonly byDate = new Map<string, DaySummary>();

  constructor(private readonly fetchDays: FetchDays) {}

  /** Fetches every row from `from` to `to`, in ranges the server accepts. */
  async load(from: string, to: string): Promise<void> {
    for (const [rangeFrom, rangeTo] of splitRange(from, to)) {
      for (const row of await this.fetchDays(rangeFrom, rangeTo)) this.upsert(row);
    }
  }

  upsert(row: DaySummary): void {
    if (row.samples > 0) this.byDate.set(row.date, row);
  }

  dates(): string[] {
    return [...this.byDate.keys()].sort();
  }

  between(from: string, to: string): DaySummary[] {
    return this.dates()
      .filter((date) => date >= from && date <= to)
      .flatMap((date) => this.byDate.get(date) ?? []);
  }

  earlier(date: string): string | null {
    return this.dates().filter((candidate) => candidate < date).pop() ?? null;
  }

  later(date: string): string | null {
    return this.dates().find((candidate) => candidate > date) ?? null;
  }

  latest(): DaySummary | null {
    const dates = this.dates();
    return dates.length ? (this.byDate.get(dates[dates.length - 1]) ?? null) : null;
  }
}
