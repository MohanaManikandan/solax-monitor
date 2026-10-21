import uPlot from "uplot";
import { formatNumber } from "../format";
import { axisSize } from "./day-chart";
import { axisFont, axisTextSize, colour, FAINT, HAIRLINE, pixelRatio, SUN, WELL } from "./theme";

const ALERT = "#ff5f7e";

/** Bar widths: 70% of each slot, never wider than 40 CSS pixels. */
const BAR_SHARE = 0.7;
const WIDEST_BAR = 40;

export interface BarOptions {
  /** The readout's name for a slot, such as "Day". */
  slotName: string;
  /** How a slot's label reads on the axis and in the readout. */
  labelOf(label: number): string;
  unit: string;
  decimals: number;
  onPick(index: number): void;
}

export interface Outline {
  index: number;
  low: number;
  high: number;
}

/** One bar per day or month in solar yellow. A slot's projected range can
 * show as an outline above its bar, and faint dots under bars mark slots
 * with a fault. */
export class BarChart {
  private plot: uPlot | null = null;
  private outline: Outline | null = null;
  private markers: number[] = [];
  private drawnKey = "";
  private width = 0;
  private height = 0;

  constructor(private readonly element: HTMLElement, private readonly options: BarOptions) {
    element.style.background = WELL;
  }

  /** Draws the bars, unless they are exactly what is already drawn: a
   * screen repaints on every reading, while a bar moves far less often. */
  setData(labels: number[], values: Array<number | null>, outline: Outline | null, markers: number[]): void {
    const key = JSON.stringify([labels, values, outline, markers]);
    if (this.plot && key === this.drawnKey) return;
    this.drawnKey = key;
    this.outline = outline;
    this.markers = markers;
    const data: uPlot.AlignedData = [labels, values];
    if (!this.plot || this.plot.data[0].length !== labels.length) this.build(data);
    else this.plot.setData(data);
  }

  resize(width: number, height: number): void {
    if (width === this.width && height === this.height) return;
    this.width = width;
    this.height = height;
    this.plot?.setSize({ width, height });
  }

  destroy(): void {
    this.plot?.destroy();
    this.plot = null;
  }

  private build(data: uPlot.AlignedData): void {
    this.plot?.destroy();
    const { options } = this;
    const width = this.width || this.element.clientWidth || 640;
    const height = this.height || 240;
    const textSize = axisTextSize(width);
    const slotWidth = (plot: uPlot) => plot.bbox.width / Math.max(1, plot.data[0].length);
    const barWidth = (plot: uPlot) => Math.min(slotWidth(plot) * BAR_SHARE, WIDEST_BAR * pixelRatio());

    const plot = new uPlot(
      {
        width,
        height,
        legend: { live: true },
        cursor: { drag: { x: false, y: false, setScale: false }, points: { show: false }, y: false },
        scales: {
          x: { time: false, range: (_plot, min, max) => [min - 0.5, max + 0.5] },
          y: { range: (_plot, _min, max) => [0, Math.max(max ?? 0, this.outline?.high ?? 0, 1) * 1.08] },
        },
        axes: [
          {
            stroke: FAINT,
            font: axisFont(textSize),
            grid: { show: false },
            ticks: { show: false },
            // Room under the bars for the fault dots.
            gap: 14,
            space: textSize * 2.2,
            values: (_plot, ticks) => ticks.map((tick) => (Number.isInteger(tick) ? options.labelOf(tick) : "")),
          },
          {
            stroke: FAINT,
            font: axisFont(textSize),
            grid: { stroke: HAIRLINE, width: 1 },
            ticks: { stroke: HAIRLINE },
            values: (_plot, ticks) => ticks.map((tick) => formatNumber(tick, 0) + " " + options.unit),
            size: axisSize,
          },
        ],
        series: [
          { label: options.slotName, value: (_plot, value) => (value === null || value === undefined ? "–" : options.labelOf(value)) },
          {
            label: options.unit,
            stroke: SUN,
            fill: colour(SUN, 0.85),
            width: 0,
            paths: uPlot.paths.bars!({ size: [BAR_SHARE, WIDEST_BAR] }),
            points: { show: false },
            value: (_plot, value) => formatNumber(value, options.decimals),
          },
        ],
        hooks: {
          draw: [
            (chart) => {
              const context = chart.ctx;
              const ratio = pixelRatio();
              context.save();
              const outline = this.outline;
              if (outline && outline.index < chart.data[0].length) {
                const middle = chart.valToPos(chart.data[0][outline.index], "x", true);
                const half = barWidth(chart) / 2;
                const top = chart.valToPos(outline.high, "y", true);
                const bottom = chart.valToPos(outline.low, "y", true);
                context.strokeStyle = colour(SUN, 0.6);
                context.lineWidth = ratio;
                context.setLineDash([3 * ratio, 3 * ratio]);
                context.strokeRect(middle - half, top, half * 2, bottom - top);
                context.setLineDash([]);
              }
              context.fillStyle = colour(ALERT, 0.7);
              for (const index of this.markers) {
                if (index >= chart.data[0].length) continue;
                const middle = chart.valToPos(chart.data[0][index], "x", true);
                context.beginPath();
                context.arc(middle, chart.bbox.top + chart.bbox.height + 6 * ratio, 2.5 * ratio, 0, Math.PI * 2);
                context.fill();
              }
              context.restore();
            },
          ],
        },
      },
      data,
      this.element,
    );
    this.plot = plot;
    // The page check finds a bar's position through it, as for day charts.
    Object.assign(this.element, { solaxPlot: plot });
    plot.over.addEventListener("click", () => {
      const index = plot.cursor.idx;
      if (index !== null && index !== undefined) options.onPick(index);
    });
  }
}
