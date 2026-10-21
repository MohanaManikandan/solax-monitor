import uPlot from "uplot";
import type { Metric } from "../api/bindings";
import type { DayData } from "../data/day-data";
import { formatNumber } from "../format";
import type { ProjectionPath } from "../projection-path";
import { alignWithProjection } from "./align";
import { ChartGroup, type GroupMember } from "./group";
import { seriesName, type Pane } from "./panes";
import { ceilingPlugin, peakBandHeight, peakPlugin } from "./plugins";
import { operatingRange } from "./range";
import { rebuildNeeded } from "./redraw";
import { stretchesPlugin } from "./stretches";
import { axisFont, axisTextSize, colour, FAINT, HAIRLINE, pixelRatio, WELL } from "./theme";
import { defaultWindow } from "./window";

const CURSOR_SYNC_KEY = "solax-day";

/** Space a chart takes before its screen has sized it. */
const FALLBACK_HEIGHT = 320;

type Column = Array<number | null>;

export interface DayChartOptions {
  isMain: boolean;
  heroKey: string;
  ratingWatts: number;
  /** The most the inverter delivers, drawn as its limit on the power chart. */
  limitWatts: number;
  timezone: string;
  /** A mouse can drag to zoom; on a touch screen the same drag scrolls. */
  fineZoom: boolean;
  isToday(date: string): boolean;
  shown(metric: Metric): boolean;
  onSeriesToggle(key: string, shown: boolean): void;
  peaksWhileProducing(metric: Metric): boolean;
  /** Charts that share this one's window and zoom; alone when absent. */
  group?: ChartGroup;
}

/** The parts of an axis uPlot fills in once the chart exists. */
interface ResolvedAxis {
  _size: number;
  gap: number;
  ticks: { size: number };
  font: [string];
}

function axisOptions(textSize: number, extra: uPlot.Axis): uPlot.Axis {
  return {
    stroke: FAINT,
    grid: { stroke: HAIRLINE, width: 1 },
    ticks: { stroke: HAIRLINE },
    font: axisFont(textSize),
    ...extra,
  };
}

// Tick labels carry their unit. With an axis on each side, a bare number
// would leave the reader to work out which side is which.
function tickValues(unit: string): uPlot.Axis.Values {
  return (_plot, ticks, _axisIndex, _space, increment) => {
    const decimals = Math.min(3, (String(increment || 1).split(".")[1] || "").length);
    return ticks.map((tick) => (tick === null || tick === undefined ? "" : formatNumber(tick, decimals) + (unit ? " " + unit : "")));
  };
}

// A unit makes the labels wider than the fixed gutter uPlot reserves, so each
// value axis is sized to its longest label. The later passes repeat the first
// answer, which lets the layout settle instead of oscillating.
export function axisSize(plot: uPlot, values: string[], axisIndex: number, cycle: number): number {
  const axis = plot.axes[axisIndex] as unknown as ResolvedAxis;
  if (cycle > 1) return axis._size;
  let size = axis.ticks.size + axis.gap;
  const longest = (values || []).reduce((widest, value) => (value.length > widest.length ? value : widest), "");
  if (longest) {
    plot.ctx.font = axis.font[0];
    size += plot.ctx.measureText(longest).width / (uPlot.pxRatio || 1);
  }
  return Math.ceil(size);
}

/**
 * One pane's lines over one local day. On today's chart a pane holding the
 * day's yield also draws where the yield is heading: a dashed line from the
 * latest reading to the end of a typical day, over a faint band from the
 * low to the high figure.
 */
export class DayChart implements GroupMember {
  private plot: uPlot | null = null;
  private data: DayData | null = null;
  private projection: ProjectionPath | null = null;
  /** The hero's column on the plot's time axis, for the producing rules. */
  private producingColumn: Column | null = null;
  private drawnLatest: number | null = null;
  /** The charts this one is read against; they share its window and zoom. */
  private readonly group: ChartGroup;
  private width = 0;
  private height = 0;
  /** Where the day's yield sits among the pane's metrics, or -1. */
  private readonly yieldPosition: number;

  constructor(
    private readonly element: HTMLElement,
    private readonly pane: Pane,
    private readonly options: DayChartOptions,
  ) {
    this.yieldPosition = pane.metrics.findIndex((metric) => metric.summary !== null);
    this.group = options.group ?? new ChartGroup();
    this.group.join(this);
    element.dataset.chart = pane.name;
    element.style.background = WELL;
  }

  /** Shows the window another chart in the group moved to. */
  followWindow(min: number, max: number): void {
    const view = this.plot?.scales.x;
    if (!this.plot || !view || (view.min === min && view.max === max)) return;
    this.plot.setScale("x", { min, max });
  }

  /** Draws a day. A different day, or a projection appearing or going,
   * builds the chart afresh; more readings for the same day redraw it and
   * keep any zoom, following the live edge unless zoomed away from it. */
  setData(data: DayData, projection: ProjectionPath | null): void {
    const drawn = this.yieldPosition >= 0 ? projection : null;
    const rebuild =
      this.plot === null ||
      rebuildNeeded(this.data && { date: this.data.date, projected: this.projection !== null }, { date: data.date, projected: drawn !== null });
    const previousLatest = this.drawnLatest;
    const view = this.plot ? { min: this.plot.scales.x.min, max: this.plot.scales.x.max } : null;
    this.data = data;
    this.projection = drawn;
    this.shareProjectionEnd();
    this.drawnLatest = data.latest();
    const plotData = this.plotData();
    if (rebuild || this.plot === null || view === null) {
      this.build(plotData);
      return;
    }

    const following = previousLatest === null || (view.max ?? -Infinity) >= previousLatest || this.group.followsLive;
    // A plain setData snaps the chart back to the extent of its data, which
    // would throw away a zoom on each reading. The data goes in with the
    // scales held, and setting the window afterwards redraws and refits the
    // value axes.
    this.plot.setData(plotData, false);
    if (!this.group.zoomed || view.min === null || view.min === undefined || view.max === null || view.max === undefined) {
      this.resetWindow();
    } else {
      this.plot.setScale("x", { min: view.min, max: following ? Math.max(Date.now() / 1000, this.drawnLatest ?? 0) : view.max });
    }
  }

  resize(width: number, height: number): void {
    this.width = width;
    this.height = height;
    this.plot?.setSize({ width, height });
  }

  /** Back to the window the chart opens on, dropping any zoom. */
  resetWindow(): void {
    if (!this.plot || !this.data) return;
    this.group.zoomed = false;
    this.group.followsLive = false;
    const data = this.data;
    const hero = data.column(this.options.heroKey);
    this.plot.setScale(
      "x",
      defaultWindow({
        timestamps: data.timestamps,
        producing: hero.length ? hero : null,
        isToday: this.options.isToday(data.date),
        dayStart: data.from,
        nowSeconds: Date.now() / 1000,
        // Shared, so every chart in the group opens on the same window.
        projectionEnd: this.group.projectionEnd,
      }),
    );
  }

  /** Tells the group where this chart's projection ends while it is shown,
   * or that none is shown. Only the chart that draws the day's yield has a
   * say. */
  private shareProjectionEnd(): void {
    if (this.yieldPosition < 0) return;
    const path = this.projection;
    const shown = this.options.shown(this.pane.metrics[this.yieldPosition]);
    this.group.projectionEnd = path && shown ? path.timestamps[path.timestamps.length - 1] : null;
  }

  /** Switches a line, as its legend entry does. */
  setSeriesShown(key: string, shown: boolean): void {
    const position = this.pane.metrics.findIndex((metric) => metric.key === key);
    if (position < 0) return;
    if (this.plot) this.plot.setSeries(position + 1, { show: shown });
    else this.options.onSeriesToggle(key, shown);
  }

  destroy(): void {
    this.group.leave(this);
    this.plot?.destroy();
    this.plot = null;
  }

  private plotData(): uPlot.AlignedData {
    const data = this.data!;
    const columns = this.pane.metrics.map((metric) => data.column(metric.key));
    const hero = data.column(this.options.heroKey);
    const heroColumn = hero.length ? hero : null;
    if (!this.projection) {
      this.producingColumn = heroColumn;
      return [data.timestamps, ...columns] as uPlot.AlignedData;
    }
    const withHero = heroColumn ? [heroColumn, ...columns] : columns;
    const aligned = alignWithProjection(data.timestamps, withHero, this.projection);
    this.producingColumn = heroColumn ? aligned.columns[0] : null;
    const paneColumns = heroColumn ? aligned.columns.slice(1) : aligned.columns;
    return [aligned.timestamps, ...paneColumns, ...aligned.projected] as uPlot.AlignedData;
  }

  private build(plotData: uPlot.AlignedData): void {
    this.plot?.destroy();
    // A new day opens unzoomed, for the whole group.
    this.group.zoomed = false;
    this.group.followsLive = false;
    const { pane, options } = this;
    const width = this.width || this.element.clientWidth || 640;
    const height = this.height || FALLBACK_HEIGHT;
    const textSize = axisTextSize(width);
    const valueText = (metric: Metric) => (_self: uPlot, value: number | null) =>
      value === null || value === undefined ? formatNumber(null, 0) : formatNumber(value, metric.decimals) + " " + metric.unit;

    const series: uPlot.Series[] = [{}];
    pane.metrics.forEach((metric, position) => {
      const stroke = pane.colours[position];
      const isHero = metric.key === options.heroKey;
      series.push({
        label: seriesName(metric),
        scale: pane.scaleOf(metric),
        stroke,
        // On the main chart the hero leads and the rest sit a little lighter.
        // Beneath it no series outranks another.
        width: isHero || !options.isMain ? 2 : 1.5,
        fill: isHero ? colour(stroke, 0.1) : "transparent",
        show: options.shown(metric),
        value: valueText(metric),
        spanGaps: false,
      });
    });

    const bands: uPlot.Band[] = [];
    const firstProjected = series.length;
    if (this.projection) {
      const yieldMetric = pane.metrics[this.yieldPosition];
      const stroke = pane.colours[this.yieldPosition];
      const scale = pane.scaleOf(yieldMetric);
      const show = options.shown(yieldMetric);
      const ratio = pixelRatio();
      const value = valueText(yieldMetric);
      series.push({ label: "Projected", scale, stroke, width: 2, dash: [6 * ratio, 6 * ratio], points: { show: false }, show, value, spanGaps: false });
      series.push({ label: "Projected low", scale, stroke: "transparent", width: 0, points: { show: false }, show, value, spanGaps: false });
      series.push({ label: "Projected high", scale, stroke: "transparent", width: 0, points: { show: false }, show, value, spanGaps: false });
      bands.push({ series: [firstProjected + 2, firstProjected + 1], fill: colour(stroke, 0.12) });
    }

    // Time ticks are spaced for their labels. A window of a few hours earns
    // quarter-hour ticks, and uPlot's default spacing ran "10:45am" into the
    // label beside it.
    const axes: uPlot.Axis[] = [
      axisOptions(textSize, { space: () => textSize * 6 }),
      axisOptions(textSize, { scale: "y", values: tickValues(pane.units[0]), size: axisSize }),
    ];
    // The right axis draws no grid of its own: two sets of lines at unrelated
    // spacings would cross-hatch the plot.
    if (pane.units.length > 1) {
      axes.push(axisOptions(textSize, { scale: "y2", side: 1, grid: { show: false }, values: tickValues(pane.units[1]), size: axisSize }));
    }

    const producing = () => this.producingColumn;
    // The ceiling is the inverter's rating in watts, so it belongs only on the
    // chart that reads the hero's power, against the hero's own axis.
    const plugins: uPlot.Plugin[] = [
      peakPlugin(pane, { textSize, timezone: options.timezone, producing, peaksWhileProducing: options.peaksWhileProducing }),
    ];
    const hero = pane.metrics.find((metric) => metric.key === options.heroKey);
    if (options.isMain && hero && hero.unit === "W") {
      plugins.unshift(ceilingPlugin(options.limitWatts, pane.scaleOf(hero), textSize));
      // The power chart is where both stretches show as a shape: a flat top
      // at the limit, a step down when the inverter runs hot.
      plugins.unshift(stretchesPlugin(() => this.data?.stretches ?? null, { watts: options.limitWatts, scale: pane.scaleOf(hero) }));
    }

    const scales: uPlot.Scales = { x: { time: true } };
    for (const scale of ["y", "y2"] as const) {
      const gated = pane.metrics.some((metric) => options.peaksWhileProducing(metric) && pane.scaleOf(metric) === scale);
      if (gated) {
        scales[scale] = {
          range: operatingRange(pane, scale, {
            ratingWatts: options.ratingWatts,
            zoomed: () => this.group.zoomed,
            producing,
            peaksWhileProducing: options.peaksWhileProducing,
          }),
        };
      }
    }

    // A strip above the plot holds the peak labels, so they never sit on the
    // curve. A chart with nothing to mark keeps the usual edge.
    const hasPeaks = pane.metrics.some((metric) => metric.peak);
    const padding: uPlot.Padding | undefined = hasPeaks ? [() => peakBandHeight(textSize), null, null, null] : undefined;

    const plot = new uPlot(
      {
        width,
        height,
        plugins,
        padding,
        scales,
        axes,
        series,
        bands,
        // Times read in the plant's timezone wherever the screen is.
        tzDate: (ts) => uPlot.tzDate(new Date(ts * 1000), options.timezone),
        // The crosshair's time is shared between day charts, its level is
        // not. A drag shorter than a few pixels is a click that slipped, not
        // a zoom.
        cursor: {
          drag: { x: options.fineZoom, y: false, dist: 5 },
          sync: {
            key: CURSOR_SYNC_KEY,
            setSeries: false,
            match: [(ownScale, otherScale) => ownScale === otherScale, () => false],
          },
        },
        hooks: {
          // uPlot reports the selection when the button is released, just
          // before it applies the zoom. A drag let go within a couple of
          // pixels of the right edge of a view at the live edge keeps
          // following new readings.
          setSelect: [
            (chart) => {
              if (chart.select.width <= 0) return;
              this.group.zoomed = true;
              const latest = this.data?.latest() ?? null;
              this.group.followsLive =
                chart.select.left + chart.select.width >= chart.bbox.width / pixelRatio() - 2 &&
                latest !== null &&
                (chart.scales.x.max ?? -Infinity) >= latest;
            },
          ],
          // A window this chart moved to, by a zoom or the live edge, is
          // shown by the rest of its group. A chart still being built
          // reports its starting range; that is not passed on.
          setScale: [
            (chart, key) => {
              if (key !== "x" || chart.status !== 1) return;
              const { min, max } = chart.scales.x;
              if (min === null || min === undefined || max === null || max === undefined) return;
              this.group.moved(this, min, max);
            },
          ],
          setSeries: [(chart, index, changed) => this.seriesChanged(chart, index, changed)],
        },
      },
      plotData,
      this.element,
    );
    this.plot = plot;
    // Lets the page check read what the chart draws.
    Object.assign(this.element, { solaxPlot: plot });

    // The projection's three series follow the yield line's own legend
    // entry, so they stay out of the readout under the chart.
    const legendRows = plot.root.querySelectorAll<HTMLElement>(".u-legend .u-series");
    for (let index = firstProjected; index < series.length; index++) {
      const row = legendRows[index];
      if (row) row.style.display = "none";
    }

    // uPlot's own double click resets to the extent of the data, readings
    // from before generation included. This runs after it and puts back the
    // window every chart in the group opened on.
    plot.over.addEventListener("dblclick", () => this.group.reset());
    this.resetWindow();
  }

  private seriesChanged(chart: uPlot, index: number | null, changed: uPlot.Series | undefined): void {
    if (index === null || index < 1 || !changed || typeof changed.show !== "boolean") return;
    // A projection series switched along with its line below.
    if (index > this.pane.metrics.length) return;
    this.options.onSeriesToggle(this.pane.metrics[index - 1].key, changed.show);
    if (index - 1 === this.yieldPosition && this.projection) {
      for (let offset = 0; offset < 3; offset++) chart.setSeries(this.pane.metrics.length + 1 + offset, { show: changed.show });
      // The projection appearing or going changes where the day's window
      // ends, for every chart read against this one.
      this.shareProjectionEnd();
      if (!this.group.zoomed) this.group.reset();
    }
  }
}
