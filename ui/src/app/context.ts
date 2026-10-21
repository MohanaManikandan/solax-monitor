import type { Metric, Registry } from "../api/bindings";
import type { Transport } from "../api/transport";
import type { Capacity } from "../figures";

/** Facts fixed for the visit, read once from the registry. */
export interface Context {
  transport: Transport;
  registry: Registry;
  metricsByKey: Map<string, Metric>;
  hero: Metric;
  runMode: Metric;
  dailyYield: Metric;
  chartKeys: string[];
  timezone: string;
  ratingWatts: number;
  /** The most the inverter delivers, which can sit above its rating. */
  limitWatts: number;
  /** What is measured against that limit: the power the inverter
   * delivers, or the hero when the registry does not name it. */
  limitMetric: Metric;
  /** Counts down to the grid connection, when the registry names one. */
  gridCountdown: Metric | null;
  intervalSeconds: number;
  co2KgPerKwh: number;
  /** What specific yield divides by. */
  capacity: Capacity;
}

export function contextFrom(transport: Transport, registry: Registry): Context {
  const metricsByKey = new Map(registry.metrics.map((metric) => [metric.key, metric]));
  const named = (key: string): Metric => {
    const metric = metricsByKey.get(key);
    if (!metric) throw new Error(`the registry names ${key}, which it does not list`);
    return metric;
  };
  return {
    transport,
    registry,
    metricsByKey,
    hero: named(registry.roles.hero),
    runMode: named(registry.roles.run_mode),
    dailyYield: named(registry.roles.daily_yield),
    chartKeys: registry.metrics.filter((metric) => metric.chart).map((metric) => metric.key),
    timezone: registry.site.timezone,
    ratingWatts: Math.round(registry.site.rating_kw * 1000),
    // An older server sends no limit; the rating stands in for it.
    limitWatts: Math.round((registry.site.max_output_kw || registry.site.rating_kw) * 1000),
    // An older server names no output.
    limitMetric: named(registry.roles.output ?? registry.roles.hero),
    gridCountdown: registry.roles.grid_countdown ? named(registry.roles.grid_countdown) : null,
    intervalSeconds: registry.site.poll_interval_seconds,
    co2KgPerKwh: registry.site.co2_kg_per_kwh,
    capacity: registry.site.panel_kwp ? { kw: registry.site.panel_kwp, of: "panels" } : { kw: registry.site.rating_kw, of: "inverter" },
  };
}
