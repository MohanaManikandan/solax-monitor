import { expect, test } from "vitest";
import { yearLines } from "./year";

/** The text with no-break spaces, which keep it on one line. */
const unbroken = (text: string) => text.replaceAll(" ", "\u00a0");

const options = { decimals: 1, unit: "kWh", heroUnit: "W", timezone: "Asia/Kolkata", locale: "en-GB" };

test("the year's figures are tiles: its best month, its day records, then the lifetime figures", () => {
  const lines = yearLines(
    {
      totalKwh: 227.2,
      months: [],
      bestMonth: { month: "2026-09", totalKwh: 160.2, daysRecorded: 9 },
      co2Kg: 149.4,
      lifetimeKwh: 2081.1,
      lifetimeCo2Kg: 1477.6,
      best: { date: "2026-09-26", kwh: 19.9 },
      lowest: { date: "2026-09-23", kwh: 3 },
      peak: { date: "2026-09-22", watts: 3447, at: Date.UTC(2026, 8, 22, 6, 41) / 1000 },
      specificYield: { kwhPerKw: 62, per: "panels" },
      limitSeconds: 19_800,
      mostAtLimit: { date: "2026-09-26", seconds: 7140 },
      derateCount: 26,
      derateSeconds: 15_600,
      mostDerated: { date: "2026-09-26", count: 6 },
    },
    options,
  );
  expect(lines.total).toBe("227.2 kWh");
  expect(lines.tiles).toEqual([
    { label: "Best month", value: "160.2 kWh", detail: "September" },
    { label: "Best day", value: "19.9 kWh", detail: unbroken("26 Sept") },
    { label: "Lowest day", value: "3.0 kWh", detail: unbroken("23 Sept") },
    { label: "Peak power", value: "3,447 W", detail: unbroken("22 Sept") + " 12:11" },
    { label: "At the inverter's limit", value: unbroken("5 h 30 min"), detail: `Most on ${unbroken("26 Sept")}, ${unbroken("1 h 59 min")}` },
    { label: "Heat derating", value: "26 times", detail: `${unbroken("4 h 20 min")} in all, most on ${unbroken("26 Sept")}` },
    { label: "Specific yield", value: "62 kWh/kWp", detail: "Per kWp of panels" },
    { label: "CO2 avoided", value: "149 kg", detail: null },
    { label: "Lifetime yield", value: "2,081.1 kWh", detail: "Since the inverter was installed" },
    { label: "Lifetime CO2 avoided", value: "1,478 kg", detail: "Since the inverter was installed" },
  ]);
});
