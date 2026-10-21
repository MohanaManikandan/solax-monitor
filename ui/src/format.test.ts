import { describe, expect, test } from "vitest";
import { formatBytes, formatDuration, formatNumber, localDate, timeOfDay } from "./format";

/** The text with no-break spaces, which keep it on one line. */
const unbroken = (text: string) => text.replaceAll(" ", "\u00a0");

test("a duration reads in hours and minutes, dropping whichever is not needed", () => {
  expect(formatDuration(7140)).toBe(unbroken("1 h 59 min"));
  expect(formatDuration(2460)).toBe(unbroken("41 min"));
  expect(formatDuration(7200)).toBe(unbroken("2 h"));
  expect(formatDuration(20)).toBe(unbroken("0 min"));
});

test("sizes on disk read in the largest whole unit, as a file manager shows them", () => {
  expect(formatBytes(500, "en-GB")).toBe("500 B");
  expect(formatBytes(2048, "en-GB")).toBe("2 KB");
  expect(formatBytes(38_965_112, "en-GB")).toBe("37.2 MB");
  expect(formatBytes(5_368_709_120, "en-GB")).toBe("5.0 GB");
});

describe("formatNumber", () => {
  test("rounds to the declared decimals with grouping", () => {
    expect(formatNumber(1234.56, 1, "en-US")).toBe("1,234.6");
    expect(formatNumber(6, 1, "en-US")).toBe("6.0");
  });
  test("shows an en dash for a missing value and passes labels through", () => {
    expect(formatNumber(null, 1)).toBe("–");
    expect(formatNumber(undefined, 1)).toBe("–");
    expect(formatNumber("Normal", 0)).toBe("Normal");
  });
});

describe("dates and times in the plant's timezone", () => {
  test("a reading just after local midnight belongs to the new local day", () => {
    // 2026-10-03 18:30:05 UTC is 2026-10-04 00:00:05 in Kolkata.
    const ts = Date.UTC(2026, 9, 3, 18, 30, 5) / 1000;
    expect(localDate(ts, "Asia/Kolkata")).toBe("2026-10-04");
    expect(localDate(ts, "UTC")).toBe("2026-10-03");
  });
  test("times read in the plant's timezone", () => {
    const ts = Date.UTC(2026, 9, 4, 5, 11, 0) / 1000;
    expect(timeOfDay(ts, "Asia/Kolkata", "en-GB")).toBe("10:41");
  });
});
