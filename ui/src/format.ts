const MISSING = "–";

export function formatNumber(value: number | string | null | undefined, decimals: number, locale?: string): string {
  if (value === null || value === undefined) return MISSING;
  if (typeof value === "string") return value;
  return value.toLocaleString(locale, { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
}

const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB"];

/** A size on disk in the largest unit it fills, counting in 1,024s as file
 * managers do: whole bytes and kilobytes, a tenth of anything larger. */
export function formatBytes(bytes: number, locale?: string): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < SIZE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return formatNumber(value, unit >= 2 ? 1 : 0, locale) + " " + SIZE_UNITS[unit];
}

/** A span of time in hours and whole minutes, such as "1 h 59 min". */
export function formatDuration(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  // No-break spaces: a duration is one figure and never wraps in two.
  if (!hours) return `${rest}\u00a0min`;
  return rest ? `${hours}\u00a0h\u00a0${rest}\u00a0min` : `${hours}\u00a0h`;
}

/** The local calendar day of a reading, YYYY-MM-DD, in the plant's timezone. */
export function localDate(ts: number, timezone: string): string {
  return new Intl.DateTimeFormat("en-CA", { timeZone: timezone, year: "numeric", month: "2-digit", day: "2-digit" }).format(new Date(ts * 1000));
}

/** Hours and minutes of a reading in the plant's timezone. */
export function timeOfDay(ts: number, timezone: string, locale?: string): string {
  return new Date(ts * 1000).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", timeZone: timezone });
}
