import type { Controller } from "../app/controller";
import type { AppState } from "../app/state";

export interface DayNav {
  readonly element: HTMLElement;
  paint(): void;
}

/**
 * Arrows, a date picker and Today, stepping through the days that hold
 * readings. Home and Details share one selected day, so each carries this
 * control and moving on one moves the other. The arrow keys step too, on
 * whichever screen is showing.
 */
export function createDayNav(state: AppState, controller: Controller, visible: () => boolean): DayNav {
  const element = document.createElement("div");
  element.className = "day-nav";
  element.innerHTML = `
    <button type="button" class="day-prev" title="Previous day with readings">&lsaquo;</button>
    <input type="date" class="day-picker" aria-label="Day to show">
    <button type="button" class="day-next" title="Next day with readings">&rsaquo;</button>
    <button type="button" class="day-today text">Today</button>
  `;
  const find = <Found extends HTMLElement>(selector: string) => element.querySelector(selector) as Found;
  const previous = find<HTMLButtonElement>(".day-prev");
  const next = find<HTMLButtonElement>(".day-next");
  const today = find<HTMLButtonElement>(".day-today");
  const picker = find<HTMLInputElement>(".day-picker");

  const go = (date: string | undefined) => {
    if (date) void controller.selectDay(date);
  };
  previous.addEventListener("click", () => go(previous.dataset.target));
  next.addEventListener("click", () => go(next.dataset.target));
  today.addEventListener("click", () => go(state.today.today));
  picker.addEventListener("change", () => go(picker.value));
  document.addEventListener("keydown", (event) => {
    if (!visible() || event.target instanceof HTMLInputElement) return;
    if (event.key === "ArrowLeft") go(previous.dataset.target);
    if (event.key === "ArrowRight") go(next.dataset.target);
  });

  return {
    element,
    paint() {
      const known = state.days.dates();
      picker.value = state.selectedDate;
      picker.max = state.today.today;
      if (known.length) picker.min = known[0];
      const earlier = controller.previousDay();
      const later = controller.nextDay();
      previous.disabled = earlier === null;
      next.disabled = later === null;
      previous.dataset.target = earlier ?? "";
      next.dataset.target = later ?? "";
      // Left enabled while another day's readings are still on the chart, so
      // a failed load of today can be asked for again.
      today.disabled = state.isToday() && state.day?.date === state.today.today;
    },
  };
}
