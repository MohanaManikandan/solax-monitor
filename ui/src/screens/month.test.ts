import { expect, test } from "vitest";
import { emptyMonthMessage, monthLines } from "./month";

/** The text with no-break spaces, which keep it on one line. */
const unbroken = (text: string) => text.replaceAll(" ", "\u00a0");

const september = {
  totalKwh: 312.4, daysRecorded: 30, averageKwh: 10.413, comparedWithPrevious: 8.2,
  best: { date: "2026-09-14", kwh: 17.9 }, lowest: { date: "2026-09-03", kwh: 4.2 },
  peak: { date: "2026-09-22", watts: 3447, at: Date.UTC(2026, 8, 22, 6, 41) / 1000 },
  specificYield: { kwhPerKw: 104.13, per: "inverter" as "inverter" | "panels" }, co2Kg: 221.8, faultDays: [] as string[],
  limitSeconds: 19_800, mostAtLimit: { date: "2026-09-26", seconds: 7140 },
  derateCount: 26, derateSeconds: 15_600, mostDerated: { date: "2026-09-26", count: 6 },
};
const options = { decimals: 1, unit: "kWh", heroUnit: "W", timezone: "Asia/Kolkata", locale: "en-GB" };

test("the month's summary reads as the design shows it", () => {
  const lines = monthLines(september, "August", options);
  expect(lines.total).toBe("312.4 kWh");
  expect(lines.average).toBe("10.4 kWh/day");
  expect(lines.comparison).toBe("8% more per day than August");
});

test("each figure is a tile led by its value, with its detail underneath", () => {
  // en-GB writes September short as "Sept", as Node's ICU prints it.
  expect(monthLines(september, "August", options).tiles).toEqual([
    { label: "Best day", value: "17.9 kWh", detail: unbroken("14 Sept") },
    { label: "Lowest day", value: "4.2 kWh", detail: unbroken("3 Sept") },
    { label: "Peak power", value: "3,447 W", detail: unbroken("22 Sept") + " 12:11" },
    { label: "At the inverter's limit", value: unbroken("5 h 30 min"), detail: `Most on ${unbroken("26 Sept")}, ${unbroken("1 h 59 min")}` },
    { label: "Heat derating", value: "26 times", detail: `${unbroken("4 h 20 min")} in all, most on ${unbroken("26 Sept")}` },
    { label: "Specific yield", value: "104 kWh/kW", detail: "Per kW of inverter rating" },
    { label: "CO2 avoided", value: "222 kg", detail: null },
    { label: "Days recorded", value: "30", detail: null },
  ]);
});

test("a figure with nothing to show leaves its tile out, and faults get one when there are any", () => {
  const labels = monthLines({ ...september, best: null, lowest: null, limitSeconds: null, mostAtLimit: null, faultDays: ["2026-09-24", "2026-09-25"] }, "August", options).tiles.map(
    (tile) => `${tile.label}: ${tile.value}`,
  );
  expect(labels).not.toContain("Best day: 17.9 kWh");
  expect(labels.some((label) => label.startsWith("At the inverter's limit"))).toBe(false);
  expect(labels).toContain("Faults: 2 days");
});

test("specific yield names what it is per: the panels when their capacity is known", () => {
  const perPanel = monthLines({ ...september, specificYield: { kwhPerKw: 85.2, per: "panels" } }, "August", options).tiles.find(
    (tile) => tile.label === "Specific yield",
  );
  expect(perPanel).toEqual({ label: "Specific yield", value: "85 kWh/kWp", detail: "Per kWp of panels" });
});

test("heat derating counts in times, and a month without any says so plainly", () => {
  const derating = (count: number, seconds: number) =>
    monthLines({ ...september, derateCount: count, derateSeconds: seconds, mostDerated: count ? { date: "2026-09-24", count } : null }, "August", options).tiles.find(
      (tile) => tile.label === "Heat derating",
    );
  expect(derating(1, 540)).toEqual({ label: "Heat derating", value: "1 time", detail: `${unbroken("9 min")} in all, most on ${unbroken("24 Sept")}` });
  expect(derating(0, 0)).toEqual({ label: "Heat derating", value: "0 times", detail: null });
  const unnamed = monthLines({ ...september, derateCount: null, derateSeconds: null, mostDerated: null }, "August", options).tiles;
  expect(unnamed.some((tile) => tile.label === "Heat derating")).toBe(false);
});

test("a month without readings says so and offers the nearest month that has some", () => {
  expect(emptyMonthMessage("2026-03", ["2026-09", "2026-10"], "en-GB")).toEqual({
    sentence: "No readings in March 2026.",
    nearest: "2026-09",
  });
});
