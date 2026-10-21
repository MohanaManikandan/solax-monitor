import type { DayRecords } from "../figures";
import { formatDuration, formatNumber, timeOfDay } from "../format";
import type { Tile } from "./period";

export interface RecordOptions { decimals: number; unit: string; heroUnit: string; timezone: string; locale?: string }

/** A day such as "14 Sept", held on one line like any other figure. */
export function dayLabel(date: string, locale?: string): string {
  return new Date(date + "T00:00:00Z").toLocaleDateString(locale, { day: "numeric", month: "short", timeZone: "UTC" }).replaceAll(" ", "\u00a0");
}

/** Specific yield in kWh per kWp of panels, or per kW of the inverter's
 * rating when the panels' capacity is not configured. */
function specificYieldTile(specificYield: NonNullable<DayRecords["specificYield"]>, options: RecordOptions): Tile {
  const value = formatNumber(specificYield.kwhPerKw, 0, options.locale);
  return specificYield.per === "panels"
    ? { label: "Specific yield", value: `${value} ${options.unit}/kWp`, detail: "Per kWp of panels" }
    : { label: "Specific yield", value: `${value} ${options.unit}/kW`, detail: "Per kW of inverter rating" };
}

/** The tiles a month and a year share, in the order shown. A record with
 * nothing to show is left out. */
export function recordTiles(records: DayRecords, options: RecordOptions): Tile[] {
  const energy = (value: number) => `${formatNumber(value, options.decimals, options.locale)} ${options.unit}`;
  const day = (date: string) => dayLabel(date, options.locale);
  const { best, lowest, peak, limitSeconds, mostAtLimit, derateCount, derateSeconds, mostDerated, specificYield } = records;
  const tiles: Array<Tile | null> = [
    best && { label: "Best day", value: energy(best.kwh), detail: day(best.date) },
    lowest && { label: "Lowest day", value: energy(lowest.kwh), detail: day(lowest.date) },
    peak && {
      label: "Peak power",
      value: `${formatNumber(peak.watts, 0, options.locale)} ${options.heroUnit}`,
      detail: day(peak.date) + (peak.at === null ? "" : " " + timeOfDay(peak.at, options.timezone, options.locale)),
    },
    limitSeconds === null
      ? null
      : {
          label: "At the inverter's limit",
          value: formatDuration(limitSeconds),
          detail: mostAtLimit && mostAtLimit.seconds > 0 ? `Most on ${day(mostAtLimit.date)}, ${formatDuration(mostAtLimit.seconds)}` : null,
        },
    derateCount === null
      ? null
      : {
          label: "Heat derating",
          value: `${derateCount} ${derateCount === 1 ? "time" : "times"}`,
          detail: mostDerated ? `${formatDuration(derateSeconds ?? 0)} in all, most on ${day(mostDerated.date)}` : null,
        },
    specificYield === null ? null : specificYieldTile(specificYield, options),
  ];
  return tiles.filter((tile) => tile !== null);
}
