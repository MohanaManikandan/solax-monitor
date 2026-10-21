import { describe, expect, test } from "vitest";
import type { Day } from "../api/bindings";
import { DayData, gapThreshold, insertGaps } from "./day-data";

function day(ts: number[], bucketSeconds: number | null = null): Day {
  return {
    date: "2026-10-04", from: 0, to: 86_399, count: ts.length, bucket_seconds: bucketSeconds, ts,
    series: { pv_power: ts.map((_, index) => index * 10) }, peaks: {}, closing: {},
    stretches: { at_limit: [], derating: [] },
  };
}

test("a day keeps its stretches, and today's are replaced as the live stream reports them", () => {
  const data = DayData.fromDay({ ...day([0, 5]), stretches: { at_limit: [[0, 5]], derating: [] } }, ["pv_power"], 5);
  expect(data.stretches).toEqual({ at_limit: [[0, 5]], derating: [] });
  data.setStretches({ at_limit: [[0, 10]], derating: [[20, 600]] });
  expect(data.stretches).toEqual({ at_limit: [[0, 10]], derating: [[20, 600]] });
});

describe("gaps", () => {
  test("the threshold covers the poll rate and, when downsampled, twice the bucket", () => {
    expect(gapThreshold(5, null)).toBe(60);
    expect(gapThreshold(30, null)).toBe(90);
    expect(gapThreshold(5, 288)).toBe(576);
  });
  test("a step longer than the threshold gets a break", () => {
    const [timestamps, columns] = insertGaps([0, 5, 200], [[1, 2, 3]], 60);
    expect(timestamps).toEqual([0, 5, 6, 200]);
    expect(columns[0]).toEqual([1, 2, null, 3]);
  });
  test("a day read every five minutes draws as one line, and breaks only after three missing readings", () => {
    const data = DayData.fromDay(day([0, 300, 600, 900, 1800, 2700]), ["pv_power"], 5);
    expect(data.timestamps).toEqual([0, 300, 600, 900, 1800, 2700]);
    const sparse = DayData.fromDay(day([0, 300, 600, 900, 2100]), ["pv_power"], 5);
    expect(sparse.timestamps).toEqual([0, 300, 600, 900, 901, 2100]);
  });
  test("a day polled every few seconds still breaks after a minute without readings", () => {
    const data = DayData.fromDay(day([0, 5, 10, 15, 90]), ["pv_power"], 5);
    expect(data.timestamps).toEqual([0, 5, 10, 15, 16, 90]);
  });
});

test("a day keeps every metric's value at its last reading", () => {
  const data = DayData.fromDay({ ...day([0, 5]), closing: { today_kwh: 19.5, run_mode: "Normal" } }, ["pv_power"], 5);
  expect(data.closing).toEqual({ today_kwh: 19.5, run_mode: "Normal" });
});

describe("appending live readings", () => {
  test("a reading at or before the latest is ignored, so a catch-up never draws one twice", () => {
    const data = DayData.fromDay(day([0, 5, 10]), ["pv_power"], 5);
    expect(data.append(10, { pv_power: 99 })).toBe(false);
    expect(data.append(15, { pv_power: 30 })).toBe(true);
    expect(data.timestamps).toEqual([0, 5, 10, 15]);
    expect(data.column("pv_power")).toEqual([0, 10, 20, 30]);
  });
  test("a reading after an outage starts after a break", () => {
    const data = DayData.fromDay(day([0, 5]), ["pv_power"], 5);
    data.append(500, { pv_power: 7 });
    expect(data.timestamps).toEqual([0, 5, 6, 500]);
    expect(data.column("pv_power")).toEqual([0, 10, null, 7]);
  });
});
