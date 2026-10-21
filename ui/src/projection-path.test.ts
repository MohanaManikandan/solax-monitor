import { expect, test } from "vitest";
import type { Curve } from "./api/bindings";
import { pathKey, projectionPath } from "./projection-path";

// A day producing evenly from 06:00 to 18:00: the share rises in a straight
// line between those boundaries.
function evenCurve(): Curve {
  const shares = Array.from({ length: 289 }, (_, boundary) => Math.min(1, Math.max(0, (boundary * 300 - 6 * 3600) / (12 * 3600))));
  return { date: "2026-10-04", slot_seconds: 300, based_on_days: 3, median: shares, lower: shares, upper: shares };
}

const dayStart = 1_791_052_200; // a local midnight
const noon = dayStart + 12 * 3600;

test("the path starts at the yield so far and ends at the expected total", () => {
  const path = projectionPath(evenCurve(), { state: "on_pace", expected_kwh: 20, low_kwh: 20, high_kwh: 20, based_on_days: 3 }, 10, noon, dayStart);
  expect(path).not.toBeNull();
  expect(path!.timestamps[0]).toBe(noon);
  expect(path!.expected[0]).toBe(10);
  expect(path!.expected[path!.expected.length - 1]).toBeCloseTo(20, 6);
  expect(path!.timestamps[path!.timestamps.length - 1]).toBe(dayStart + 18 * 3600);
  expect(path!.expected.every((value) => value >= 10)).toBe(true);
});

test("the band runs from the low to the high total", () => {
  const path = projectionPath(evenCurve(), { state: "on_pace", expected_kwh: 20, low_kwh: 18, high_kwh: 22, based_on_days: 3 }, 10, noon, dayStart)!;
  const last = path.timestamps.length - 1;
  expect(path.low[last]).toBeCloseTo(18, 6);
  expect(path.high[last]).toBeCloseTo(22, 6);
});

test("the same projection keys the same however many times it is built, and a change keys differently", () => {
  const onPace = { state: "on_pace", expected_kwh: 20, low_kwh: 18, high_kwh: 22, based_on_days: 3 } as const;
  const first = projectionPath(evenCurve(), { ...onPace }, 10, noon, dayStart);
  const again = projectionPath(evenCurve(), { ...onPace }, 10, noon, dayStart);
  const later = projectionPath(evenCurve(), { ...onPace }, 10.1, noon + 5, dayStart);
  expect(pathKey(first)).toBe(pathKey(again));
  expect(pathKey(later)).not.toBe(pathKey(first));
  expect(pathKey(null)).toBeNull();
});

test("there is no path unless the projection is on pace and a curve exists", () => {
  expect(projectionPath(evenCurve(), { state: "too_early", from: "08:30" }, 0.1, noon, dayStart)).toBeNull();
  const empty: Curve = { date: "2026-10-04", slot_seconds: 300, based_on_days: 0, median: [], lower: [], upper: [] };
  expect(projectionPath(empty, { state: "on_pace", expected_kwh: 20, low_kwh: 20, high_kwh: 20, based_on_days: 3 }, 10, noon, dayStart)).toBeNull();
});
