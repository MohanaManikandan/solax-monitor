import { localDate } from "../format";

/** The plant's local day. It moves forward when the clock passes midnight,
 * when the server reports a later day, or when a reading from one arrives. */
export class TodayTracker {
  constructor(public today: string, private readonly timezone: string) {}

  /** Moves today to `date` when that is later; says whether it moved. */
  advanceTo(date: string): boolean {
    if (date <= this.today) return false;
    this.today = date;
    return true;
  }

  rolledOver(ts: number): boolean {
    return this.advanceTo(localDate(ts, this.timezone));
  }
}

/** Seconds from `nowSeconds` to the next local midnight in `timezone`. */
export function secondsUntilMidnight(nowSeconds: number, timezone: string): number {
  const parts = new Intl.DateTimeFormat("en-GB", { timeZone: timezone, hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" })
    .formatToParts(new Date(nowSeconds * 1000));
  const part = (type: string) => Number(parts.find((entry) => entry.type === type)?.value ?? 0);
  const intoDay = part("hour") * 3600 + part("minute") * 60 + part("second");
  return 86_400 - intoDay;
}
