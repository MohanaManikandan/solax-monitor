import type { Curve, Projection } from "./api/bindings";

/** Allows for rounding in shares that should read exactly one. */
const WHOLE_DAY = 1 - 1e-9;

export interface ProjectionPath {
  timestamps: number[];
  expected: number[];
  low: number[];
  high: number[];
}

/** What a path draws, as one comparable value: equal keys draw the same
 * line, so a chart can skip redrawing it. */
export function pathKey(path: ProjectionPath | null): string | null {
  return path ? JSON.stringify(path) : null;
}

/**
 * The projected cumulative yield from the latest reading to the end of a
 * typical day. Each path scales one of the curve's share lines so that it
 * passes through the yield so far and ends at its projected total: the
 * median to the expected total, the 75th percentile to the low total and the
 * 25th to the high total. No point falls below the yield already made. The
 * path ends where the slowest of the three shares reaches the whole day, so
 * all three lines end on their totals.
 */
export function projectionPath(curve: Curve, projection: Projection, made: number, measuredAt: number, dayStart: number): ProjectionPath | null {
  if (projection.state !== "on_pace" || curve.median.length === 0) return null;
  const slot = curve.slot_seconds;
  const path: ProjectionPath = { timestamps: [measuredAt], expected: [made], low: [made], high: [made] };
  const firstBoundary = Math.floor((measuredAt - dayStart) / slot) + 1;
  for (let boundary = firstBoundary; boundary < curve.median.length; boundary++) {
    path.timestamps.push(dayStart + boundary * slot);
    path.expected.push(Math.max(made, projection.expected_kwh * curve.median[boundary]));
    path.low.push(Math.max(made, projection.low_kwh * curve.upper[boundary]));
    path.high.push(Math.max(made, projection.high_kwh * curve.lower[boundary]));
    if (curve.lower[boundary] >= WHOLE_DAY) break;
  }
  return path;
}
