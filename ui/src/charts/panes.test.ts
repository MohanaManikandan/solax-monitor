import { expect, test } from "vitest";
import type { Metric } from "../api/bindings";
import { buildPanes, seriesName, seriesShown } from "./panes";

function metric(key: string, chart: string | null, unit: string, section: Metric["section"], extra: Partial<Metric> = {}): Metric {
  return {
    key, label: key, unit, section, slots: { one: { index: 0, signed: false } }, scale: 1, decimals: 0,
    chart: chart ? { name: chart, hidden: false } : null, peak: null, feature: null, summary: null, note: null, ...extra,
  };
}

test("the hero's chart is the main one, the rest follow in first-appearance order", () => {
  const metrics = [
    metric("pv_volts", "voltage", "V", "solar"),
    metric("pv_power", "power", "W", "solar"),
    metric("today_kwh", "power", "kWh", "solar", { summary: "generated" }),
    metric("ac_power", "power", "W", "grid"),
    metric("inverter_temperature", "temperature", "C", "inverter"),
  ];
  const { main, others } = buildPanes(metrics, "pv_power");
  expect(main?.name).toBe("power");
  expect(main?.metrics.map((entry) => entry.key)).toEqual(["pv_power", "ac_power", "today_kwh"]);
  expect(main?.units).toEqual(["W", "kWh"]);
  expect(main?.colours).toEqual(["#ffc24d", "#4fd6c8", "#b8a6ff"]);
  expect(others.map((pane) => pane.name)).toEqual(["voltage", "temperature"]);
});

test("a series is named by its summary word, then its section, then its label", () => {
  expect(seriesName(metric("today_kwh", "power", "kWh", "solar", { summary: "generated" }))).toBe("Generated");
  expect(seriesName(metric("ac_volts", "voltage", "V", "grid"))).toBe("Grid");
  expect(seriesName(metric("inverter_temperature", "temperature", "C", "inverter", { label: "Temperature" }))).toBe("Temperature");
});

test("every line starts as the registry declares, today included, and a person's switch wins", () => {
  const yieldLine = metric("today_kwh", "power", "kWh", "solar", { chart: { name: "power", hidden: true } });
  const shownLine = metric("pv_power", "power", "W", "solar");
  const none = new Map<string, boolean>();
  expect(seriesShown(yieldLine, none)).toBe(false);
  expect(seriesShown(shownLine, none)).toBe(true);
  expect(seriesShown(yieldLine, new Map([["today_kwh", true]]))).toBe(true);
  expect(seriesShown(shownLine, new Map([["pv_power", false]]))).toBe(false);
});
