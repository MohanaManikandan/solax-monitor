import type { Context } from "../app/context";
import type { AppState } from "../app/state";
import { BarChart, type Outline } from "../charts/bars";
import { monthFigures, type MonthFigures } from "../figures";
import { formatNumber } from "../format";
import { addMonths, daysInMonth, monthName, monthOf, monthTitle, nearestMonth } from "../periods";
import { createPeriodFrame, historyErrorSentence, type Tile } from "./period";
import { recordTiles, type RecordOptions } from "./records";
import type { Screen } from "./screen";

export type MonthLineOptions = RecordOptions;

export interface MonthLines {
  total: string;
  average: string;
  comparison: string | null;
  /** The month's figures in the order they are shown; one with nothing to
   * show, such as faults on a clean month, is left out. */
  tiles: Tile[];
}

export function monthLines(figures: MonthFigures, previousMonthName: string, options: MonthLineOptions): MonthLines {
  const energy = (value: number) => `${formatNumber(value, options.decimals, options.locale)} ${options.unit}`;
  let comparison: string | null = null;
  if (figures.comparedWithPrevious !== null) {
    const percent = Math.round(Math.abs(figures.comparedWithPrevious));
    comparison =
      percent === 0
        ? `The same per day as ${previousMonthName}`
        : `${percent}% ${figures.comparedWithPrevious > 0 ? "more" : "less"} per day than ${previousMonthName}`;
  }
  const faultCount = figures.faultDays.length;
  const tiles: Array<Tile | null> = [
    ...recordTiles(figures, options),
    { label: "CO2 avoided", value: `${formatNumber(figures.co2Kg, 0, options.locale)} kg`, detail: null },
    { label: "Days recorded", value: formatNumber(figures.daysRecorded, 0, options.locale), detail: null },
    faultCount ? { label: "Faults", value: `${faultCount} ${faultCount === 1 ? "day" : "days"}`, detail: null } : null,
  ];
  return {
    total: energy(figures.totalKwh),
    average: figures.averageKwh === null ? "–" : `${energy(figures.averageKwh)}/day`,
    comparison,
    tiles: tiles.filter((tile) => tile !== null),
  };
}

/** What an empty month says, and the month its button leads to. */
export function emptyMonthMessage(month: string, monthsWithData: string[], locale?: string): { sentence: string; nearest: string | null } {
  return { sentence: `No readings in ${monthTitle(month, locale)}.`, nearest: nearestMonth(monthsWithData, month) };
}

export function createMonth(context: Context, state: AppState): Screen {
  const element = document.createElement("section");
  element.className = "screen";
  element.dataset.screen = "month";
  element.hidden = true;
  const go = (target: string) => {
    location.hash = "#/month/" + target;
  };
  const frame = createPeriodFrame(element, go);

  const yieldMetric = context.dailyYield;
  let month = monthOf(state.today.today);
  let visible = false;
  let days: string[] = [];
  const bars = new BarChart(frame.barsElement, {
    slotName: "Day",
    labelOf: (label) => String(label),
    unit: yieldMetric.unit,
    decimals: yieldMetric.decimals,
    onPick: (index) => {
      const date = days[index];
      if (!date) return;
      location.hash = "#/";
      window.dispatchEvent(new CustomEvent("solax:select-day", { detail: date }));
    },
  });
  frame.previous.addEventListener("click", () => go(addMonths(month, -1)));
  frame.next.addEventListener("click", () => go(addMonths(month, 1)));

  const paint = () => {
    const today = state.today.today;
    const monthsWithData = [...new Set(state.days.dates().map(monthOf))];
    const firstMonth = monthsWithData[0] ?? monthOf(today);
    frame.title.textContent = monthTitle(month);
    frame.previous.disabled = month <= firstMonth;
    frame.next.disabled = month >= monthOf(today);
    // Until the day rows arrive the cards keep their placeholders, rather
    // than claiming the month is empty; if they could not be loaded, the
    // summary says so.
    if (!state.historyLoaded) {
      if (state.historyError) frame.showMessage(historyErrorSentence(state.historyError), null);
      return;
    }

    days = daysInMonth(month);
    const rows = state.days.between(days[0], days[days.length - 1]);
    if (!rows.length) {
      const message = emptyMonthMessage(month, monthsWithData);
      frame.showMessage(message.sentence, message.nearest ? { target: message.nearest, label: "Go to " + monthTitle(message.nearest) } : null);
      return;
    }
    frame.showContent();

    const previousMonth = addMonths(month, -1);
    const previousDays = daysInMonth(previousMonth);
    const previousRows = state.days.between(previousDays[0], previousDays[previousDays.length - 1]);
    const lines = monthLines(monthFigures(rows, previousRows, today, context.capacity, context.co2KgPerKwh), monthName(previousMonth), {
      decimals: yieldMetric.decimals,
      unit: yieldMetric.unit,
      heroUnit: context.hero.unit,
      timezone: context.timezone,
    });
    frame.setTotal(lines.total);
    frame.average.textContent = lines.average;
    frame.setComparison(lines.comparison);
    frame.setTiles(lines.tiles);

    const byDate = new Map(rows.map((row) => [row.date, row]));
    const yields = days.map((date) => byDate.get(date)?.yield_kwh ?? null);
    const projection = state.now?.projection;
    const todayIndex = days.indexOf(today);
    // Today's bar grows with the stream; an outline above it shows where the
    // day is heading.
    const outline: Outline | null =
      todayIndex >= 0 && projection?.state === "on_pace" ? { index: todayIndex, low: projection.low_kwh, high: projection.high_kwh } : null;
    const faults = rows.filter((row) => row.fault_readings > 0).map((row) => days.indexOf(row.date));
    bars.resize(Math.max(280, frame.barsElement.clientWidth || 640), frame.barsHeight());
    bars.setData(days.map((_, index) => index + 1), yields, outline, faults);
  };

  state.subscribe(() => {
    if (visible) paint();
  });
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  window.addEventListener("resize", () => {
    if (resizeTimer !== null) clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      if (visible) paint();
    }, 150);
  });

  return {
    element,
    show(param) {
      visible = true;
      element.hidden = false;
      month = param ?? monthOf(state.today.today);
      paint();
    },
    hide() {
      visible = false;
      element.hidden = true;
    },
  };
}
