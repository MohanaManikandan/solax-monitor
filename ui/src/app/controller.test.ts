import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Curve, Day, Metric, Now, Registry } from "../api/bindings";
import type { LiveHandlers, Transport } from "../api/transport";
import { DaysIndex } from "../data/days-index";
import { TodayTracker } from "../data/today";
import { contextFrom } from "./context";
import { Controller } from "./controller";
import { AppState } from "./state";

const TIMEZONE = "Asia/Kolkata";

function metric(key: string, extra: Partial<Metric> = {}): Metric {
  return {
    key, label: key, unit: "W", section: "solar", slots: { one: { index: 0, signed: false } }, scale: 1, decimals: 0,
    chart: null, peak: null, feature: null, summary: null, note: null, ...extra,
  };
}

const registry: Registry = {
  roles: { hero: "pv_power", run_mode: "run_mode", daily_yield: "today_kwh", lifetime_yield: "total_kwh", grid_countdown: null, output: null },
  metrics: [metric("pv_power", { chart: { name: "power", hidden: false } }), metric("run_mode"), metric("today_kwh"), metric("total_kwh")],
  site: { inverter_serial: "", rating_kw: 3, max_output_kw: 3.3, timezone: TIMEZONE, poll_interval_seconds: 5, co2_kg_per_kwh: 0.71, panel_kwp: null },
};

function nowFor(date: string): Now {
  return {
    reading: null,
    peaks: {},
    today: { date, samples: 0, yield_kwh: null, peak_power_w: null, peak_at: null, producing_from: null, producing_until: null, lifetime_kwh: null, fault_readings: 0, limit_seconds: null, derate_count: null, derate_seconds: null },
    stretches: { at_limit: [], derating: [] },
    poller: { sleeping: true, resumes_at: null, window: null },
    projection: { state: "not_enough_history", days: 0 },
    stored: 0,
    earliest: null,
    storage_bytes: 0,
  };
}

/** A server whose idea of today can be moved, recording the days asked for.
 * Its history and curve routes can be made to fail. */
function fakeServer() {
  const server = { today: "2026-10-04", dayRequests: [] as string[], live: [] as LiveHandlers[], historyFails: false, curveFails: false, dayFails: false };
  const transport: Transport = {
    registry: async () => registry,
    now: async () => nowFor(server.today),
    curve: async (): Promise<Curve> => {
      if (server.curveFails) throw new Error("/curve answered 500");
      return { date: server.today, slot_seconds: 300, based_on_days: 0, median: [], lower: [], upper: [] };
    },
    day: async (date): Promise<Day> => {
      server.dayRequests.push(date);
      if (server.dayFails) throw new Error("/day answered 500");
      return { date, from: 0, to: 0, count: 0, bucket_seconds: null, ts: [], series: {}, peaks: {}, closing: {}, stretches: { at_limit: [], derating: [] } };
    },
    days: async () => {
      if (server.historyFails) throw new Error("/days answered 500");
      return [];
    },
    openLive: (handlers) => {
      server.live.push(handlers);
      return () => {};
    },
    exportUrl: (date) => "/export?" + date,
  };
  return { server, transport };
}

async function bootAt(utcMilliseconds: number, failing: { history?: boolean; curve?: boolean } = {}) {
  vi.setSystemTime(new Date(utcMilliseconds));
  const { server, transport } = fakeServer();
  server.historyFails = failing.history ?? false;
  server.curveFails = failing.curve ?? false;
  const state = new AppState(new DaysIndex((from, to) => transport.days(from, to)), new TodayTracker("2026-10-04", TIMEZONE));
  const controller = new Controller(contextFrom(transport, registry), state, () => 640);
  await controller.boot();
  return { server, state, controller };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.stubGlobal("document", { visibilityState: "visible", addEventListener: () => {} });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a page showing today moves to the new day at midnight, with no reading to prompt it", async () => {
  // 23:59 in Kolkata, with the poller asleep for the night.
  const { server, state } = await bootAt(Date.UTC(2026, 9, 4, 18, 29, 0));
  expect(state.selectedDate).toBe("2026-10-04");
  server.today = "2026-10-05";
  await vi.advanceTimersByTimeAsync(61_000);
  expect(state.today.today).toBe("2026-10-05");
  expect(state.selectedDate).toBe("2026-10-05");
  expect(state.now?.today.date).toBe("2026-10-05");
  expect(server.dayRequests.at(-1)).toBe("2026-10-05");
});

test("the server reporting a later day rolls the page over before anything reloads", async () => {
  const { server, state } = await bootAt(Date.UTC(2026, 9, 4, 4, 30, 0));
  server.live[0].event({ type: "reading", ...nowFor("2026-10-05") });
  await vi.advanceTimersByTimeAsync(0);
  expect(state.today.today).toBe("2026-10-05");
  expect(state.selectedDate).toBe("2026-10-05");
  expect(server.dayRequests).toEqual(["2026-10-04", "2026-10-05"]);
});

test("a history that fails to load still leaves today's chart and the live stream running, and is tried again on reconnecting", async () => {
  const { server, state } = await bootAt(Date.UTC(2026, 9, 4, 4, 30, 0), { history: true });
  expect(server.dayRequests).toEqual(["2026-10-04"]);
  expect(server.live.length).toBe(1);
  expect(state.historyLoaded).toBe(false);
  expect(state.historyError).toContain("/days answered 500");

  server.historyFails = false;
  server.live[0].opened();
  server.live[0].opened();
  await vi.advanceTimersByTimeAsync(0);
  expect(state.historyLoaded).toBe(true);
  expect(state.historyError).toBeNull();
});

test("today's chart that failed to load is tried again when the next reading arrives", async () => {
  vi.setSystemTime(new Date(Date.UTC(2026, 9, 4, 6, 30, 0)));
  const { server, transport } = fakeServer();
  server.dayFails = true;
  const state = new AppState(new DaysIndex((from, to) => transport.days(from, to)), new TodayTracker("2026-10-04", TIMEZONE));
  await new Controller(contextFrom(transport, registry), state, () => 640).boot();
  expect(state.loadError).toContain("/day answered 500");

  server.dayFails = false;
  const reading = { ts: Date.UTC(2026, 9, 4, 6, 30, 5) / 1000, values: {}, device: { ver: null, inverter_type: null }, health: { run_mode: null, severity: "ok" as const, fault: null } };
  server.live[0].event({ type: "reading", ...nowFor("2026-10-04"), reading });
  await vi.advanceTimersByTimeAsync(0);

  expect(state.loadError).toBeNull();
  expect(state.day?.date).toBe("2026-10-04");
});

test("today's chart takes the stretches each live reading brings", async () => {
  const { server, state } = await bootAt(Date.UTC(2026, 9, 4, 6, 30, 0));
  const reading = { ts: Date.UTC(2026, 9, 4, 6, 30, 5) / 1000, values: {}, device: { ver: null, inverter_type: null }, health: { run_mode: null, severity: "ok" as const, fault: null } };
  const stretches = { at_limit: [[100, 400]] as Array<[number, number]>, derating: [[500, 1100]] as Array<[number, number]> };
  server.live[0].event({ type: "reading", ...nowFor("2026-10-04"), reading, stretches });
  await vi.advanceTimersByTimeAsync(0);
  expect(state.day?.stretches).toEqual(stretches);
});

test("a curve that fails to load costs only the projection", async () => {
  const { server, state } = await bootAt(Date.UTC(2026, 9, 4, 4, 30, 0), { curve: true });
  expect(state.curve).toBeNull();
  expect(state.historyLoaded).toBe(true);
  expect(server.dayRequests).toEqual(["2026-10-04"]);
  expect(server.live.length).toBe(1);
});

test("a page browsing a past day stays on it across midnight", async () => {
  const { server, state, controller } = await bootAt(Date.UTC(2026, 9, 4, 18, 29, 0));
  await controller.selectDay("2026-10-03");
  server.today = "2026-10-05";
  await vi.advanceTimersByTimeAsync(61_000);
  expect(state.today.today).toBe("2026-10-05");
  expect(state.selectedDate).toBe("2026-10-03");
});
