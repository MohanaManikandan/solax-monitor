import type { Projection } from "./api/bindings";
import { formatNumber } from "./format";

/** The line under today's yield on the hero card, or null when it hides. */
export function projectionLine(projection: Projection, decimals: number, unit: string, locale?: string): string | null {
  switch (projection.state) {
    case "not_enough_history":
      return "Projection after 3 full days";
    case "too_early":
      return projection.from ? "Projection from about " + projection.from : null;
    case "on_pace": {
      const figure = (value: number) => formatNumber(value, decimals, locale);
      return `On pace for ${figure(projection.expected_kwh)} ${unit} · ${figure(projection.low_kwh)} to ${figure(projection.high_kwh)}`;
    }
    case "finished":
      return null;
  }
}
