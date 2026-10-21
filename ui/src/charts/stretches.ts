import type uPlot from "uplot";
import type { Stretches } from "../api/bindings";
import { colour, EMBER, pixelRatio, SUN } from "./theme";

/** The parts of the stretches inside the plot, as left and right edges in
 * canvas pixels. One narrower than a pixel is widened to one, so a short
 * stretch never vanishes at a wide window. */
export function visibleSpans(stretches: Array<[number, number]>, xOf: (ts: number) => number, left: number, right: number): Array<[number, number]> {
  const spans: Array<[number, number]> = [];
  for (const [start, end] of stretches) {
    const from = Math.max(left, xOf(start));
    const to = Math.min(right, xOf(end));
    if (!Number.isFinite(from) || !Number.isFinite(to) || to < left || from > right || to < from) continue;
    spans.push([from, Math.max(to, from + 1)]);
  }
  return spans;
}

/** Joins stretches less than gapSeconds apart, for drawing: a dip of a few
 * seconds under the limit would otherwise split one stretch into slivers.
 * A gap holding one of the barriers, such as a derate, is never joined. */
export function joinClose(stretches: Array<[number, number]>, gapSeconds: number, barriers: Array<[number, number]>): Array<[number, number]> {
  const joined: Array<[number, number]> = [];
  for (const [start, end] of stretches) {
    const last = joined[joined.length - 1];
    const blocked = last !== undefined && barriers.some(([from, to]) => from < start && to > last[1]);
    if (last && start - last[1] < gapSeconds && !blocked) last[1] = end;
    else joined.push([start, end]);
  }
  return joined;
}

/** Height of the solid strip along the foot of the plot, in CSS pixels. */
const STRIP_HEIGHT = 4;

/** Dips under the limit shorter than this draw as part of the stretch. */
const JOIN_SECONDS = 120;

/** Where the limit line sits, so time at the limit can be shaded above it. */
export interface LimitLine {
  watts: number;
  scale: string;
}

/**
 * Marks the stretches of the day behind the lines, each with a solid strip
 * along the plot's foot. A derate also gets a faint ember wash over the
 * plot's full height, the step down it caused sitting inside it. Time at
 * the limit gets a gold wash only above the limit line, where the power the
 * panels could have given was cut off; washed full height, its dozens of
 * short runs would stripe the whole middle of the day. Reads the stretches
 * at each draw, so today's grow without rebuilding the chart.
 */
export function stretchesPlugin(stretchesOf: () => Stretches | null, limit: LimitLine): uPlot.Plugin {
  return {
    hooks: {
      drawClear: (chart) => {
        const stretches = stretchesOf();
        if (!stretches) return;
        const { left, top, width, height } = chart.bbox;
        const xOf = (ts: number) => chart.valToPos(ts, "x", true);
        const strip = STRIP_HEIGHT * pixelRatio();
        const context = chart.ctx;
        // On an overcast day the scale can top out under the limit, and
        // then there is nothing above the line to shade.
        const limitY = chart.valToPos(limit.watts, limit.scale, true);
        const capHeight = Number.isFinite(limitY) ? Math.min(height, Math.max(0, limitY - top)) : 0;
        const atLimit = joinClose(stretches.at_limit, JOIN_SECONDS, stretches.derating);
        const kinds = [
          { spans: atLimit, hex: SUN, washHeight: capHeight, wash: 0.22 },
          { spans: stretches.derating, hex: EMBER, washHeight: height, wash: 0.2 },
        ];
        context.save();
        for (const kind of kinds) {
          for (const [from, to] of visibleSpans(kind.spans, xOf, left, left + width)) {
            if (kind.washHeight > 0) {
              context.fillStyle = colour(kind.hex, kind.wash);
              context.fillRect(from, top, to - from, kind.washHeight);
            }
            context.fillStyle = colour(kind.hex, 0.85);
            context.fillRect(from, top + height - strip, to - from, strip);
          }
        }
        context.restore();
      },
    },
  };
}
