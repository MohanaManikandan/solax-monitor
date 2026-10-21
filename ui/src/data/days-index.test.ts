import { expect, test } from "vitest";
import type { DaySummary } from "../api/bindings";
import { DaysIndex } from "./days-index";

function row(date: string): DaySummary {
  return { date, samples: 10, yield_kwh: 1, peak_power_w: 1, peak_at: 0, producing_from: null, producing_until: null, lifetime_kwh: null, fault_readings: 0, limit_seconds: null, derate_count: null, derate_seconds: null };
}

test("a long history is fetched in ranges of at most 400 days and reaches the first day", async () => {
  const asked: Array<[string, string]> = [];
  const index = new DaysIndex(async (from, to) => {
    asked.push([from, to]);
    return [from, to].filter((date) => date === "2024-01-01" || date === "2026-10-04").map(row);
  });
  await index.load("2024-01-01", "2026-10-04");
  expect(asked.length).toBe(3);
  expect(index.dates()).toEqual(["2024-01-01", "2026-10-04"]);
  expect(index.earlier("2026-10-04")).toBe("2024-01-01");
  expect(index.later("2024-01-01")).toBe("2026-10-04");
});

test("today's row is replaced as it changes, and a row without readings is not listed", () => {
  const index = new DaysIndex(async () => []);
  index.upsert({ ...row("2026-10-04"), yield_kwh: 1 });
  index.upsert({ ...row("2026-10-04"), yield_kwh: 2 });
  index.upsert({ ...row("2026-10-05"), samples: 0 });
  expect(index.between("2026-10-01", "2026-10-31").map((entry) => entry.yield_kwh)).toEqual([2]);
});
