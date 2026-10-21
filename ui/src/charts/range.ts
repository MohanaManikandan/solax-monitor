import uPlot from "uplot";
import type { Metric } from "../api/bindings";
import type { Pane } from "./panes";

// Below this share of the rating the inverter is still finding its operating
// point. A start-up begins at a few watts with PV voltage near 40 V and takes
// several minutes and about 50 W to settle in the 205 to 255 V band.
const OPERATING_SHARE = 0.02;

export interface RangeOptions {
  ratingWatts: number;
  zoomed(): boolean;
  /** The hero's column on the chart's time axis, when the day has one. */
  producing(): Array<number | null> | null;
  peaksWhileProducing(metric: Metric): boolean;
}

// Fits a value axis to the readings worth reading. A metric whose peak counts
// only while producing is fitted from readings past start-up, so the minutes
// of PV voltage near 40 V no longer squash the operating band into a thin
// line. Those readings are still drawn, below the visible range. A zoom shows
// everything in view, since zooming there is asking to see it.
export function operatingRange(pane: Pane, scale: string, options: RangeOptions): uPlot.Range.Function {
  return (plot, dataMin, dataMax) => {
    // Nothing showing on this axis, which hides the axis.
    if (dataMin === null || dataMin === undefined) return [null, null];
    const fitted = uPlot.rangeNum(dataMin, dataMax, 0.1, true);
    const power = options.producing();
    if (options.zoomed() || !power) return fitted;

    // The visible stretch. During a change of window this is already the new
    // one, while the scale itself still holds the old.
    const [first, last] = plot.series[0].idxs || [];
    if (first === null || first === undefined || last === null || last === undefined) return fitted;

    // The top reaches every producing reading, the same rule as the peak
    // marker, so the marker always lands on the plot. Only the bottom waits
    // for the plant to pass start-up, which is where the 40 V readings sit.
    const floor = options.ratingWatts * OPERATING_SHARE;
    const extentOf = (values: ArrayLike<number | null | undefined>, gated: boolean): [number, number] => {
      let extentLow = Infinity;
      let extentHigh = -Infinity;
      for (let index = first; index <= last; index++) {
        const value = values[index];
        if (value === null || value === undefined) continue;
        const watts = power[index] ?? 0;
        if (gated && !(watts > 0)) continue;
        if (value > extentHigh) extentHigh = value;
        if (gated && !(watts >= floor)) continue;
        if (value < extentLow) extentLow = value;
      }
      return [extentLow, extentHigh];
    };

    let low = Infinity;
    let high = -Infinity;
    pane.metrics.forEach((metric, position) => {
      if (pane.scaleOf(metric) !== scale || !plot.series[position + 1].show) return;
      const values = plot.data[position + 1];
      let [metricLow, metricHigh] = extentOf(values, options.peaksWhileProducing(metric));
      // Nothing in view is past start-up yet, or only a single reading, as
      // in the first minutes of the morning, so this line is fitted as it
      // is. Left out, it would vanish below an axis fitted to the other line
      // alone, and one reading alone would stretch the axis around itself.
      if (options.peaksWhileProducing(metric) && !(metricLow < metricHigh)) {
        [metricLow, metricHigh] = extentOf(values, false);
      }
      if (metricLow < low) low = metricLow;
      if (metricHigh > high) high = metricHigh;
    });
    return low <= high ? uPlot.rangeNum(low, high, 0.1, true) : fitted;
  };
}
