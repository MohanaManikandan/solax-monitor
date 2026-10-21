import { describe, expect, test } from "vitest";
import { addDays, addMonths, daysInMonth, monthName, monthOf, monthTitle, nearestMonth, nearestYear, splitRange } from "./periods";

describe("calendar arithmetic", () => {
  test("adds days across month and year ends", () => {
    expect(addDays("2026-09-30", 1)).toBe("2026-10-01");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
  });
  test("adds months and lists a month's days", () => {
    expect(addMonths("2026-01", -1)).toBe("2025-12");
    expect(monthOf("2026-09-22")).toBe("2026-09");
    expect(daysInMonth("2028-02")).toHaveLength(29);
  });
  test("names a month with and without its year", () => {
    expect(monthTitle("2026-03", "en-GB")).toBe("March 2026");
    expect(monthName("2026-09", "en-GB")).toBe("September");
  });
});

describe("splitRange", () => {
  test("keeps a short range whole", () => {
    expect(splitRange("2026-09-01", "2026-09-30")).toEqual([["2026-09-01", "2026-09-30"]]);
  });
  test("splits a long history into ranges of at most 400 days that meet without overlap", () => {
    const ranges = splitRange("2024-01-01", "2026-10-04");
    expect(ranges[0]).toEqual(["2024-01-01", "2025-02-03"]);
    expect(ranges.every(([from, to]) => from <= to)).toBe(true);
    for (let index = 1; index < ranges.length; index++) {
      expect(ranges[index][0]).toBe(addDays(ranges[index - 1][1], 1));
    }
    expect(ranges[ranges.length - 1][1]).toBe("2026-10-04");
  });
});

describe("nearest period with data", () => {
  test("picks the closest month, preferring the earlier on a tie", () => {
    expect(nearestMonth(["2026-09", "2026-10"], "2026-03")).toBe("2026-09");
    expect(nearestMonth(["2026-07", "2026-09"], "2026-08")).toBe("2026-07");
    expect(nearestMonth([], "2026-08")).toBeNull();
  });
  test("picks the closest year", () => {
    expect(nearestYear(["2026"], "2024")).toBe("2026");
  });
});
