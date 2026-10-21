import { describe, expect, test } from "vitest";
import type { DaySummary } from "./api/bindings";
import { monthFigures, yearFigures, type Capacity } from "./figures";

const INVERTER: Capacity = { kw: 3, of: "inverter" };

function row(date: string, yieldKwh: number | null, extra: Partial<DaySummary> = {}): DaySummary {
  return {
    date, samples: yieldKwh === null ? 0 : 5000, yield_kwh: yieldKwh, peak_power_w: 3000, peak_at: 0,
    producing_from: null, producing_until: null, lifetime_kwh: null, fault_readings: 0, limit_seconds: null,
    derate_count: null, derate_seconds: null, ...extra,
  };
}

describe("monthFigures", () => {
  const today = "2026-09-25";
  const rows = [row("2026-09-23", 3), row("2026-09-24", 18.9, { peak_power_w: 3447, peak_at: 99, fault_readings: 2 }), row("2026-09-25", 6)];
  const previous = [row("2026-08-30", 10), row("2026-08-31", 12)];
  const figures = monthFigures(rows, previous, today, INVERTER, 0.71);

  test("totals today's live figure with the completed days", () => {
    expect(figures.totalKwh).toBeCloseTo(27.9, 9);
    expect(figures.daysRecorded).toBe(3);
    expect(figures.averageKwh).toBeCloseTo(9.3, 9);
  });
  test("best and lowest come from completed days only", () => {
    expect(figures.best).toEqual({ date: "2026-09-24", kwh: 18.9 });
    expect(figures.lowest).toEqual({ date: "2026-09-23", kwh: 3 });
  });
  test("peak, specific yield, CO2, comparison and fault days", () => {
    expect(figures.peak).toEqual({ date: "2026-09-24", watts: 3447, at: 99 });
    expect(figures.specificYield?.kwhPerKw).toBeCloseTo(9.3, 9);
    expect(figures.specificYield?.per).toBe("inverter");
    expect(figures.co2Kg).toBeCloseTo(27.9 * 0.71, 9);
    expect(figures.comparedWithPrevious).toBeCloseTo(((9.3 - 11) / 11) * 100, 9);
    expect(figures.faultDays).toEqual(["2026-09-24"]);
  });
  test("time at the limit adds up over the month and names its longest day", () => {
    const limited = monthFigures(
      [row("2026-09-24", 18.9, { limit_seconds: 2460 }), row("2026-09-26", 22.5, { limit_seconds: 7140 }), row("2026-09-27", 20.1, { limit_seconds: null })],
      [],
      today,
      INVERTER,
      0.71,
    );
    expect(limited.limitSeconds).toBe(9600);
    expect(limited.mostAtLimit).toEqual({ date: "2026-09-26", seconds: 7140 });
  });
  test("with the panels' capacity known, specific yield is per kWp of panels", () => {
    const perPanel = monthFigures(rows, previous, today, { kw: 3.665, of: "panels" }, 0.71).specificYield;
    expect(perPanel?.kwhPerKw).toBeCloseTo(27.9 / 3.665, 9);
    expect(perPanel?.per).toBe("panels");
  });
  test("without a configured limit there is no time at it", () => {
    expect(figures.limitSeconds).toBeNull();
    expect(figures.mostAtLimit).toBeNull();
  });
  test("heat derating adds up over the month and names the day it happened most", () => {
    const derated = monthFigures(
      [
        row("2026-09-24", 18.9, { derate_count: 1, derate_seconds: 540 }),
        row("2026-09-26", 22.5, { derate_count: 6, derate_seconds: 3600 }),
        row("2026-09-27", 20.1, { derate_count: 5, derate_seconds: 3420 }),
      ],
      [],
      today,
      INVERTER,
      0.71,
    );
    expect(derated.derateCount).toBe(12);
    expect(derated.derateSeconds).toBe(7560);
    expect(derated.mostDerated).toEqual({ date: "2026-09-26", count: 6 });
  });
  test("rows from a server that predates the limit and derating figures give none, not NaN", () => {
    const older = [row("2026-09-24", 18.9), row("2026-09-25", 6)].map((day) => {
      const { limit_seconds, derate_count, derate_seconds, ...rest } = day;
      return rest as DaySummary;
    });
    const figuresOfOlder = monthFigures(older, [], today, INVERTER, 0.71);
    expect(figuresOfOlder.limitSeconds).toBeNull();
    expect(figuresOfOlder.derateCount).toBeNull();
  });
  test("without the metrics that show heat derating there is no figure for it", () => {
    expect(figures.derateCount).toBeNull();
    expect(figures.mostDerated).toBeNull();
  });
  test("an empty month has no averages, extremes or comparison", () => {
    const empty = monthFigures([], previous, today, INVERTER, 0.71);
    expect(empty.totalKwh).toBe(0);
    expect(empty.averageKwh).toBeNull();
    expect(empty.best).toBeNull();
    expect(empty.comparedWithPrevious).toBeNull();
  });
});

describe("yearFigures", () => {
  test("totals each month, picks the best and carries the lifetime counter", () => {
    const rows = [row("2026-09-22", 17.7), row("2026-09-23", 3), row("2026-10-01", 17.8)];
    const figures = yearFigures(rows, 2026, "2026-10-04", INVERTER, 0.71, 2081.1);
    expect(figures.months).toHaveLength(12);
    expect(figures.months[8]).toEqual({ month: "2026-09", totalKwh: 20.7, daysRecorded: 2 });
    expect(figures.bestMonth?.month).toBe("2026-09");
    expect(figures.totalKwh).toBeCloseTo(38.5, 9);
    expect(figures.lifetimeKwh).toBe(2081.1);
    expect(figures.lifetimeCo2Kg).toBeCloseTo(2081.1 * 0.71, 9);
  });
  test("carries the same day records as a month, over the whole year", () => {
    const rows = [
      row("2026-09-22", 17.7, { peak_power_w: 3447, peak_at: 99, limit_seconds: 600, derate_count: 0, derate_seconds: 0 }),
      row("2026-09-26", 19.9, { limit_seconds: 7140, derate_count: 6, derate_seconds: 3600 }),
      row("2026-10-03", 3, { limit_seconds: 0, derate_count: 1, derate_seconds: 600 }),
    ];
    const figures = yearFigures(rows, 2026, "2026-10-04", { kw: 3.665, of: "panels" }, 0.71, null);
    expect(figures.best).toEqual({ date: "2026-09-26", kwh: 19.9 });
    expect(figures.lowest).toEqual({ date: "2026-10-03", kwh: 3 });
    expect(figures.peak).toEqual({ date: "2026-09-22", watts: 3447, at: 99 });
    expect(figures.limitSeconds).toBe(7740);
    expect(figures.mostAtLimit).toEqual({ date: "2026-09-26", seconds: 7140 });
    expect(figures.derateCount).toBe(7);
    expect(figures.mostDerated).toEqual({ date: "2026-09-26", count: 6 });
    expect(figures.specificYield?.kwhPerKw).toBeCloseTo(40.6 / 3.665, 9);
  });
});
