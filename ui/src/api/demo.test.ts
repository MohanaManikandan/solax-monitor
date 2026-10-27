import { expect, test, vi } from "vitest";
import { addDays } from "../periods";
import { localDate } from "../format";
import { DemoTransport } from "./demo";

const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";

function today(): string {
  return localDate(Date.now() / 1000, timezone);
}

function daysAgo(count: number): string {
  return addDays(today(), -count);
}

test("demo registry uses the regular UI schema and browser timezone", async () => {
  const transport = new DemoTransport();
  const registry = await transport.registry();

  expect(registry.site.inverter_serial).toBe("X.X.X");
  expect(registry.site.max_output_kw).toBe(3.3);
  expect(registry.site.timezone).toBe(timezone);
  expect(registry.roles.hero).toBe("pv_power");
  expect(registry.metrics.some((metric) => metric.key === "inverter_temperature")).toBe(true);
});

test("live values use the browser clock and receive a fresh timestamp", async () => {
  const transport = new DemoTransport();
  const result = await transport.now();

  expect(result.reading).not.toBeNull();
  if (!result.reading) throw new Error("Expected a synthetic demo reading");
  expect(Math.abs(Date.now() - result.reading.ts * 1000)).toBeLessThan(2_000);
  expect(result.reading.device.ver).toBe("x.x.x");
  expect(result.reading.values.pv_power).toBeGreaterThanOrEqual(0);
  expect(result.reading.values.pv_power).toBeLessThanOrEqual(3300);
  expect(result.today.date).toBe(today());
});

test("historical data covers a full year and repeats deterministically", async () => {
  const transport = new DemoTransport();
  const end = today();
  const start = addDays(end, -364);
  const rows = await transport.days(start, end);

  expect(rows).toHaveLength(365);
  expect(rows[0]?.date).toBe(start);
  expect(rows.at(-1)?.date).toBe(end);

  const historicalDate = daysAgo(12);
  const first = await transport.day(historicalDate);
  const second = await new DemoTransport().day(historicalDate);
  expect(first).toEqual(second);
  expect(first.ts.length).toBeGreaterThanOrEqual(276);
  expect(first.ts.length).toBeLessThanOrEqual(300);
  expect(first.ts).toHaveLength(first.count);
  expect(first.series.pv_power).toHaveLength(first.ts.length);
});

test("cloud passes vary by date but remain repeatable for each date", async () => {
  const transport = new DemoTransport();
  const firstDate = daysAgo(12);
  const secondDate = daysAgo(13);
  const [first, sameDateAgain, anotherDay] = await Promise.all([
    transport.day(firstDate),
    new DemoTransport().day(firstDate),
    transport.day(secondDate),
  ]);

  expect(first.series.pv_power).toEqual(sameDateAgain.series.pv_power);
  expect(first.series.pv_power).not.toEqual(anotherDay.series.pv_power);
});

test("the past month includes thermal derating and output-limit plateaus", async () => {
  const transport = new DemoTransport();
  const rows = await transport.days(daysAgo(30), today());
  const recentRows = rows.slice(-31);
  const hotSummary = recentRows.find((row) => (row.derate_count ?? 0) > 0);

  expect(hotSummary).toBeDefined();
  expect(recentRows.some((row) => (row.limit_seconds ?? 0) > 0)).toBe(true);

  const hotDay = await transport.day(hotSummary!.date);
  expect(hotDay.peaks.inverter_temperature).toBe(85);
  expect(hotDay.peaks.secondary_temperature).toBe(69);
  expect(hotDay.peaks.pv_power).toBeLessThanOrEqual(3300);
  expect(hotDay.stretches.derating.length).toBeGreaterThan(1);
  expect(hotDay.stretches.derating.every(([from, to]) => to - from <= 10 * 60)).toBe(true);
  expect(hotDay.series.pv_power).toContain(2400);

  const primaryTemperatures = hotDay.series.inverter_temperature ?? [];
  const secondaryTemperatures = hotDay.series.secondary_temperature ?? [];
  expect(primaryTemperatures.filter((temperature) => temperature === 85).length).toBeLessThanOrEqual(10);
  expect(secondaryTemperatures.filter((temperature) => temperature === 69).length).toBeLessThanOrEqual(10);
  expect(new Set(primaryTemperatures).size).toBeGreaterThan(10);

  const limitDaySummary = recentRows.find((row) => row.peak_power_w === 3300);
  expect(limitDaySummary).toBeDefined();
  const limitDay = await transport.day(limitDaySummary!.date);
  expect(limitDay.series.pv_power).toContain(3300);
  expect(limitDay.stretches.at_limit.length).toBeGreaterThan(0);
});

test("lifetime yield accumulates actual synthetic daily generation", async () => {
  const transport = new DemoTransport();
  const todayDate = today();
  const yesterdayDate = daysAgo(1);
  const [yesterday, current] = await Promise.all([
    transport.days(yesterdayDate, yesterdayDate),
    transport.days(todayDate, todayDate),
  ]);

  const previous = yesterday[0];
  const latest = current[0];
  expect(previous).toBeDefined();
  expect(latest).toBeDefined();
  if (!previous || !latest) throw new Error("Expected lifetime summaries for both dates");

  // The increase should be today's generated energy, not a fixed daily increment.
  expect(Math.abs(
    (latest.lifetime_kwh ?? 0) - (previous.lifetime_kwh ?? 0) - (latest.yield_kwh ?? 0),
  )).toBeLessThanOrEqual(0.2);
  expect((latest.lifetime_kwh ?? 0)).toBeGreaterThan(previous.lifetime_kwh ?? 0);
});

test("the typical-day curve has boundaries only through 18:00", async () => {
  const curve = await new DemoTransport().curve();

  expect(curve.slot_seconds).toBe(300);
  expect(curve.median).toHaveLength(217);
  expect(curve.lower).toHaveLength(curve.median.length);
  expect(curve.upper).toHaveLength(curve.median.length);
  expect(curve.median.at(-1)).toBe(1);
  expect(curve.lower.at(-1)).toBe(1);
  expect(curve.upper.at(-1)).toBe(1);
});

test("CSV export is generated locally", async () => {
  const transport = new DemoTransport();
  const url = transport.exportUrl(daysAgo(2));
  expect(url.startsWith("blob:")).toBe(true);
});

test("demo reads and live updates do not use fetch", async () => {
  const fetchSpy = vi.spyOn(globalThis, "fetch");
  try {
    const transport = new DemoTransport();
    await transport.registry();
    await transport.now();
    await transport.curve();
    await transport.day(daysAgo(1));
    await transport.days(daysAgo(7), today());

    const close = transport.openLive({
      event: () => undefined,
      opened: () => undefined,
      failed: () => undefined,
    });
    close();
    await Promise.resolve();

    expect(fetchSpy).not.toHaveBeenCalled();
  } finally {
    fetchSpy.mockRestore();
  }
});
