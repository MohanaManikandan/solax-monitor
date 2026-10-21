import { expect, test } from "vitest";
import type { Metric, Registry } from "../api/bindings";
import type { Transport } from "../api/transport";
import { contextFrom } from "./context";

function metric(key: string): Metric {
  return {
    key, label: key, unit: "W", section: "solar", slots: { one: { index: 0, signed: false } }, scale: 1, decimals: 0,
    chart: null, peak: null, feature: null, summary: null, note: null,
  };
}

function registry(output: string | null): Registry {
  return {
    roles: { hero: "pv_power", run_mode: "pv_power", daily_yield: "pv_power", lifetime_yield: "pv_power", grid_countdown: null, output },
    metrics: [metric("pv_power"), metric("ac_power")],
    site: { inverter_serial: "", rating_kw: 3, max_output_kw: 3.3, timezone: "Asia/Kolkata", poll_interval_seconds: 5, co2_kg_per_kwh: 0.71, panel_kwp: null },
  };
}

test("a server that sends no output limit has the rating stand in for it", () => {
  const older = registry(null);
  older.site.max_output_kw = null;
  expect(contextFrom({} as Transport, older).limitWatts).toBe(3000);
  expect(contextFrom({} as Transport, registry(null)).limitWatts).toBe(3300);
});

test("the limit is measured on the power delivered when the registry names it, and on the hero otherwise", () => {
  const transport = {} as Transport;
  expect(contextFrom(transport, registry("ac_power")).limitMetric.key).toBe("ac_power");
  expect(contextFrom(transport, registry(null)).limitMetric.key).toBe("pv_power");
});
