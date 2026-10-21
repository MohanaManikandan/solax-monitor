import type { Context } from "../app/context";
import type { AppState } from "../app/state";
import { BarChart } from "../charts/bars";
import { yearFigures, type YearFigures } from "../figures";
import { formatNumber } from "../format";
import { monthName, nearestYear } from "../periods";
import { createPeriodFrame, historyErrorSentence, type Tile } from "./period";
import { recordTiles, type RecordOptions } from "./records";
import type { Screen } from "./screen";

export interface YearLines { total: string; tiles: Tile[] }

/** Lifetime figures come from the inverter's own counter, which predates
 * this server's history, so they say so. */
const SINCE_INSTALLED = "Since the inverter was installed";

export function yearLines(figures: YearFigures, options: RecordOptions): YearLines {
  const energy = (value: number) => `${formatNumber(value, options.decimals, options.locale)} ${options.unit}`;
  const mass = (value: number) => `${formatNumber(value, 0, options.locale)} kg`;
  const { bestMonth, lifetimeKwh, lifetimeCo2Kg } = figures;
  const tiles: Array<Tile | null> = [
    bestMonth && { label: "Best month", value: energy(bestMonth.totalKwh), detail: monthName(bestMonth.month, options.locale) },
    ...recordTiles(figures, options),
    { label: "CO2 avoided", value: mass(figures.co2Kg), detail: null },
    lifetimeKwh === null ? null : { label: "Lifetime yield", value: energy(lifetimeKwh), detail: SINCE_INSTALLED },
    lifetimeCo2Kg === null ? null : { label: "Lifetime CO2 avoided", value: mass(lifetimeCo2Kg), detail: SINCE_INSTALLED },
  ];
  return { total: energy(figures.totalKwh), tiles: tiles.filter((tile) => tile !== null) };
}

function shortMonth(monthNumber: number): string {
  return new Date(Date.UTC(2000, monthNumber - 1, 1)).toLocaleDateString(undefined, { month: "short", timeZone: "UTC" });
}

export function createYear(context: Context, state: AppState): Screen {
  const element = document.createElement("section");
  element.className = "screen";
  element.dataset.screen = "year";
  element.hidden = true;
  const go = (target: string) => {
    location.hash = "#/year/" + target;
  };
  const frame = createPeriodFrame(element, go);

  const yieldMetric = context.dailyYield;
  let year = state.today.today.slice(0, 4);
  let visible = false;
  let months: string[] = [];
  const bars = new BarChart(frame.barsElement, {
    slotName: "Month",
    labelOf: shortMonth,
    unit: yieldMetric.unit,
    decimals: yieldMetric.decimals,
    onPick: (index) => {
      if (months[index]) location.hash = "#/month/" + months[index];
    },
  });

  frame.previous.addEventListener("click", () => go(String(Number(year) - 1)));
  frame.next.addEventListener("click", () => go(String(Number(year) + 1)));

  const paint = () => {
    const currentYear = state.today.today.slice(0, 4);
    const years = [...new Set(state.days.dates().map((date) => date.slice(0, 4)))];
    frame.title.textContent = year;
    frame.previous.disabled = year <= (years[0] ?? currentYear);
    frame.next.disabled = year >= currentYear;
    // Until the day rows arrive the cards keep their placeholders, rather
    // than claiming the year is empty; if they could not be loaded, the
    // summary says so.
    if (!state.historyLoaded) {
      if (state.historyError) frame.showMessage(historyErrorSentence(state.historyError), null);
      return;
    }

    const rows = state.days.between(year + "-01-01", year + "-12-31");
    if (!rows.length) {
      const nearest = nearestYear(years, year);
      frame.showMessage("No readings in " + year + ".", nearest ? { target: nearest, label: "Go to " + nearest } : null);
      return;
    }
    frame.showContent();

    const figuresOfYear = yearFigures(rows, Number(year), state.today.today, context.capacity, context.co2KgPerKwh, state.days.latest()?.lifetime_kwh ?? null);
    const lines = yearLines(figuresOfYear, {
      decimals: yieldMetric.decimals,
      unit: yieldMetric.unit,
      heroUnit: context.hero.unit,
      timezone: context.timezone,
    });
    frame.setTotal(lines.total);
    const dayCount = figuresOfYear.months.reduce((total, month) => total + month.daysRecorded, 0);
    frame.average.textContent = dayCount.toLocaleString() + (dayCount === 1 ? " day recorded" : " days recorded");
    frame.setComparison(null);
    frame.setTiles(lines.tiles);

    // One bar per month; the current month's grows with the stream.
    months = figuresOfYear.months.map((month) => month.month);
    const totals = figuresOfYear.months.map((month) => (month.daysRecorded ? month.totalKwh : null));
    bars.resize(Math.max(280, frame.barsElement.clientWidth || 640), frame.barsHeight());
    bars.setData(months.map((_, index) => index + 1), totals, null, []);
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
      year = param ?? state.today.today.slice(0, 4);
      paint();
    },
    hide() {
      visible = false;
      element.hidden = true;
    },
  };
}
