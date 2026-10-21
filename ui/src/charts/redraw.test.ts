import { expect, test } from "vitest";
import { rebuildNeeded } from "./redraw";

const today = { date: "2026-10-04", projected: false };

test("the same day loaded afresh, as when a hidden page catches up, keeps its chart and so its zoom", () => {
  expect(rebuildNeeded(today, { ...today })).toBe(false);
});

test("a different day, or the projection appearing or going, builds the chart afresh", () => {
  expect(rebuildNeeded(null, today)).toBe(true);
  expect(rebuildNeeded(today, { ...today, date: "2026-10-03" })).toBe(true);
  expect(rebuildNeeded(today, { ...today, projected: true })).toBe(true);
});
