import type { Metric } from "../api/bindings";
import { FAINT, SECTION_COLOURS, SPARE_COLOURS } from "./theme";

export const SECTION_TITLES: Record<string, string> = { solar: "Solar", grid: "Grid", inverter: "Inverter" };

/** The metrics drawn together on one chart. */
export interface Pane {
  name: string;
  title: string;
  /** The left axis's unit first, then the right axis's, if any. */
  units: string[];
  metrics: Metric[];
  colours: string[];
  scaleOf(metric: Metric): "y" | "y2";
}

/** Groups the charted metrics by chart name, in first-appearance order. The
 * hero's chart is the main one. */
export function buildPanes(metrics: Metric[], heroKey: string): { main: Pane | null; others: Pane[] } {
  const byName = new Map<string, Metric[]>();
  for (const metric of metrics) {
    if (!metric.chart) continue;
    const name = metric.chart.name;
    if (!byName.has(name)) byName.set(name, []);
    byName.get(name)!.push(metric);
  }
  const panes = [...byName].map(([name, declared]) => makePane(name, declared));
  const hero = metrics.find((metric) => metric.key === heroKey);
  const main = panes.find((pane) => hero?.chart && pane.name === hero.chart.name) ?? panes[0] ?? null;
  return { main, others: panes.filter((pane) => pane !== main) };
}

function makePane(name: string, declared: Metric[]): Pane {
  // The first unit reads off the left axis and a second off the right; the
  // server refuses a chart with a third. Left axis series go first, so they
  // claim their section colours before a right axis series asks.
  const units: string[] = [];
  for (const metric of declared) {
    if (!units.includes(metric.unit)) units.push(metric.unit);
  }
  const metrics = [
    ...declared.filter((metric) => metric.unit === units[0]),
    ...declared.filter((metric) => metric.unit !== units[0]),
  ];

  const used = new Set<string>();
  const colours = metrics.map((metric) => {
    const own = SECTION_COLOURS[metric.section];
    const chosen = own && !used.has(own) ? own : (SPARE_COLOURS.find((spare) => !used.has(spare)) ?? FAINT);
    used.add(chosen);
    return chosen;
  });

  return {
    name,
    title: name.charAt(0).toUpperCase() + name.slice(1).replace(/_/g, " "),
    units,
    metrics,
    colours,
    scaleOf: (metric) => (metric.unit === units[0] ? "y" : "y2"),
  };
}

// Every chart is already titled by what it measures, so a series only has to
// say which one it is. Solar and grid name themselves by section. A running
// total is named by the word its closing value is stated with, which reads
// the same on any day, where "Generated today" would not.
export function seriesName(metric: Metric): string {
  if (metric.summary) {
    return metric.summary.charAt(0).toUpperCase() + metric.summary.slice(1);
  }
  if (metric.section === "solar" || metric.section === "grid") {
    return SECTION_TITLES[metric.section];
  }
  return metric.label;
}

/** Whether a line is drawn. A line the person switched this visit stays as
 * they left it; otherwise it starts as the registry declares. The day's
 * projection follows the yield line, so it shows only once that is on. */
export function seriesShown(metric: Metric, toggles: Map<string, boolean>): boolean {
  return toggles.get(metric.key) ?? !metric.chart?.hidden;
}
