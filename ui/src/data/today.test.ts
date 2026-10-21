import { expect, test } from "vitest";
import { secondsUntilMidnight, TodayTracker } from "./today";

test("a later date moves today forward, and an earlier or the same one does not", () => {
  const tracker = new TodayTracker("2026-10-04", "Asia/Kolkata");
  expect(tracker.advanceTo("2026-10-04")).toBe(false);
  expect(tracker.advanceTo("2026-10-03")).toBe(false);
  expect(tracker.advanceTo("2026-10-05")).toBe(true);
  expect(tracker.today).toBe("2026-10-05");
});

test("midnight is counted in the plant's timezone", () => {
  // 23:59:00 in Kolkata is 18:29:00 UTC.
  expect(secondsUntilMidnight(Date.UTC(2026, 9, 4, 18, 29, 0) / 1000, "Asia/Kolkata")).toBe(60);
  expect(secondsUntilMidnight(Date.UTC(2026, 9, 4, 18, 30, 0) / 1000, "Asia/Kolkata")).toBe(86_400);
});

test("a reading after local midnight moves today forward once", () => {
  const tracker = new TodayTracker("2026-10-03", "Asia/Kolkata");
  const beforeMidnight = Date.UTC(2026, 9, 3, 18, 29, 55) / 1000;
  const afterMidnight = Date.UTC(2026, 9, 3, 18, 30, 5) / 1000;
  expect(tracker.rolledOver(beforeMidnight)).toBe(false);
  expect(tracker.rolledOver(afterMidnight)).toBe(true);
  expect(tracker.today).toBe("2026-10-04");
  expect(tracker.rolledOver(afterMidnight + 5)).toBe(false);
});
