import type uPlot from "uplot";
import type { Metric } from "../api/bindings";
import { formatNumber, timeOfDay } from "../format";
import type { Pane } from "./panes";
import { colour, EMBER, pixelRatio, WELL } from "./theme";

// Space between a peak label and the top of the plot it labels, in CSS pixels.
const PEAK_LABEL_GAP = 6;

// Peak labels match the axis ticks a step up, in CSS pixels. Sized by the
// chart rather than the window, so a big chart gets readable labels without
// them growing into the curve.
function peakLabelBox(textSize: number) {
  const size = textSize + 1;
  const verticalPadding = Math.round(size * 0.35);
  return {
    size,
    horizontalPadding: Math.round(size * 0.55),
    height: size + verticalPadding * 2,
  };
}

/** The strip reserved above a plot for its peak labels. */
export function peakBandHeight(textSize: number): number {
  return peakLabelBox(textSize).height + PEAK_LABEL_GAP + 4;
}

/** A dashed line at the most the inverter delivers, with its figure at the
 * left end. */
export function ceilingPlugin(limitWatts: number, scale: string, textSize: number): uPlot.Plugin {
  return {
    hooks: {
      draw: (chart) => {
        if (!limitWatts) return;
        const context = chart.ctx;
        const level = chart.valToPos(limitWatts, scale, true);
        // On an overcast day the scale tops out well below the limit, which
        // puts the ceiling above the plot. Drawing it there would paint over
        // the axis, so skip it instead of clamping it to a wrong position.
        if (!Number.isFinite(level) || level < chart.bbox.top || level > chart.bbox.top + chart.bbox.height) {
          return;
        }
        const ratio = pixelRatio();
        context.save();
        context.strokeStyle = EMBER;
        context.globalAlpha = 0.45;
        context.setLineDash([4 * ratio, 5 * ratio]);
        context.lineWidth = ratio;
        context.beginPath();
        context.moveTo(chart.bbox.left, level);
        context.lineTo(chart.bbox.left + chart.bbox.width, level);
        context.stroke();
        context.setLineDash([]);

        // At the left end of the line, where the default window is low and
        // clear because it opens as generation starts. A zoom can put the
        // curve there instead, so the text sits on a dark backing.
        const size = textSize * ratio;
        const label = (limitWatts / 1000).toFixed(2) + " kW limit";
        const textLeft = chart.bbox.left + 6 * ratio;
        // A ceiling at the top of the plot would put its text in the strip
        // where the peak labels go, so the text goes under the line instead.
        const textHeight = size + ratio;
        const roomAbove = level - 4 * ratio - textHeight >= chart.bbox.top;
        const textBottom = roomAbove ? level - 4 * ratio : level + 4 * ratio + textHeight;
        context.font = size + "px Inter, system-ui, sans-serif";
        const textWidth = context.measureText(label).width;
        context.globalAlpha = 1;
        context.fillStyle = colour(WELL, 0.85);
        context.fillRect(textLeft - 3 * ratio, textBottom - size - ratio, textWidth + 6 * ratio, size + 3 * ratio);
        context.globalAlpha = 0.75;
        context.fillStyle = EMBER;
        context.textAlign = "left";
        context.textBaseline = "bottom";
        context.fillText(label, textLeft, textBottom);
        context.restore();
      },
    },
  };
}

export interface PeakOptions {
  textSize: number;
  timezone: string;
  /** The hero's column on the chart's time axis, when the day has one. */
  producing(): Array<number | null> | null;
  peaksWhileProducing(metric: Metric): boolean;
}

interface PeakLabel {
  horizontalPosition: number;
  verticalPosition: number;
  text: string;
  valueText: string;
  width: number;
  colour: string;
  left: number;
}

// Mark the high point of each series that has a peak. The labels sit in a
// strip reserved above the plot, each joined to its dot by a hairline, so
// they never cover the curve.
export function peakPlugin(pane: Pane, options: PeakOptions): uPlot.Plugin {
  return {
    hooks: {
      draw: (chart) => {
        const context = chart.ctx;
        const ratio = pixelRatio();
        const box = peakLabelBox(options.textSize);
        const plotTop = chart.bbox.top;
        const plotBottom = chart.bbox.top + chart.bbox.height;

        // The same producing rule the server applies to the peak in the
        // cards. Idle panels sit at open circuit, which would otherwise put
        // the voltage marker at dawn. The marker reads the window in view,
        // so a metric counted all day can mark less than its card when its
        // high came outside the window.
        const producing = options.producing();

        context.save();
        context.font = "700 " + box.size * ratio + "px Inter, system-ui, sans-serif";
        const labelWidth = (text: string) => context.measureText(text).width + box.horizontalPadding * 2 * ratio;

        // Collected in the chart's own order, so the hero's comes first.
        const peaks: PeakLabel[] = [];
        pane.metrics.forEach((metric, position) => {
          if (!metric.peak || !chart.series[position + 1].show) return;
          const values = chart.data[position + 1];
          if (!values) return;
          const gated = options.peaksWhileProducing(metric) && producing;

          // Only within the visible window, so zooming re-reads the peak of
          // what is actually on screen.
          const scale = chart.scales.x;
          let best = -Infinity;
          let at = -1;
          for (let index = 0; index < values.length; index++) {
            const value = values[index];
            if (value === null || value === undefined) continue;
            if (gated && !((producing[index] ?? 0) > 0)) continue;
            const stamp = chart.data[0][index];
            if (stamp < (scale.min ?? -Infinity) || stamp > (scale.max ?? Infinity)) continue;
            if (value > best) {
              best = value;
              at = index;
            }
          }
          if (at < 0 || best <= 0) return;

          const horizontalPosition = chart.valToPos(chart.data[0][at], "x", true);
          const verticalPosition = chart.valToPos(best, pane.scaleOf(metric), true);
          // A peak the value axis does not reach gets no marker, the same
          // as the ceiling, rather than a dot off the plot and a hairline
          // through it.
          if (
            !Number.isFinite(horizontalPosition) ||
            !Number.isFinite(verticalPosition) ||
            verticalPosition < plotTop ||
            verticalPosition > plotBottom
          ) {
            return;
          }

          const time = timeOfDay(chart.data[0][at], options.timezone);
          const valueText = formatNumber(best, metric.decimals) + " " + metric.unit;
          const text = valueText + "  " + time;
          peaks.push({
            horizontalPosition,
            verticalPosition,
            text,
            valueText,
            width: labelWidth(text),
            colour: pane.colours[position],
            left: 0,
          });
        });

        // A row too wide for the plot may spread across the whole canvas,
        // since the strip sits above the axes as well. Still too wide, as on
        // a phone, the times go, and then any label that does not fit, the
        // hero's kept first.
        const spacing = 6 * ratio;
        const rowWidth = () => peaks.reduce((total, peak) => total + peak.width + spacing, -spacing);
        let leftEdge = chart.bbox.left;
        let rightEdge = chart.bbox.left + chart.bbox.width;
        if (rowWidth() > rightEdge - leftEdge) {
          leftEdge = 0;
          rightEdge = context.canvas.width;
          if (rowWidth() > rightEdge) {
            for (const peak of peaks) {
              peak.text = peak.valueText;
              peak.width = labelWidth(peak.text);
            }
            while (peaks.length > 1 && rowWidth() > rightEdge) peaks.pop();
          }
        }

        // One row of labels, each as close above its own dot as the others
        // allow. A forward pass pushes overlapping labels right, a backward
        // pass pulls them back inside the right edge.
        peaks.sort((first, second) => first.horizontalPosition - second.horizontalPosition);
        let reach = leftEdge;
        for (const peak of peaks) {
          peak.left = Math.max(peak.horizontalPosition - peak.width / 2, reach);
          reach = peak.left + peak.width + spacing;
        }
        let limit = rightEdge;
        for (let index = peaks.length - 1; index >= 0; index--) {
          const peak = peaks[index];
          peak.left = Math.max(leftEdge, Math.min(peak.left, limit - peak.width));
          limit = peak.left - spacing;
        }

        const height = box.height * ratio;
        const top = plotTop - (box.height + PEAK_LABEL_GAP) * ratio;
        const radius = 4 * ratio;

        for (const peak of peaks) {
          // The hairline leaves the label where it sits above the dot, or
          // from its nearer end when the label had to move aside.
          const inset = 8 * ratio;
          const anchor = Math.min(Math.max(peak.horizontalPosition, peak.left + inset), peak.left + peak.width - inset);
          context.beginPath();
          context.moveTo(anchor, top + height);
          context.lineTo(peak.horizontalPosition, peak.verticalPosition - radius - 2 * ratio);
          context.strokeStyle = colour(peak.colour, 0.55);
          context.lineWidth = ratio;
          context.stroke();

          // A ring around the dot so it reads against the line it sits on.
          context.beginPath();
          context.arc(peak.horizontalPosition, peak.verticalPosition, radius + 2 * ratio, 0, Math.PI * 2);
          context.fillStyle = colour(WELL, 0.85);
          context.fill();
          context.beginPath();
          context.arc(peak.horizontalPosition, peak.verticalPosition, radius, 0, Math.PI * 2);
          context.fillStyle = peak.colour;
          context.fill();

          context.beginPath();
          const round = Math.min(6 * ratio, height / 2);
          if (context.roundRect) {
            context.roundRect(peak.left, top, peak.width, height, round);
          } else {
            context.rect(peak.left, top, peak.width, height);
          }
          context.fillStyle = colour(WELL, 0.92);
          context.fill();
          context.strokeStyle = colour(peak.colour, 0.5);
          context.lineWidth = ratio;
          context.stroke();

          context.fillStyle = peak.colour;
          context.textAlign = "left";
          context.textBaseline = "middle";
          context.fillText(peak.text, peak.left + box.horizontalPadding * ratio, top + height / 2);
        }
        context.restore();
      },
    },
  };
}
