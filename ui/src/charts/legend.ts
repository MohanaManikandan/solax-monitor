import type { Metric } from "../api/bindings";
import { seriesName, type Pane } from "./panes";

/**
 * The legend above a day chart. Each entry also switches its line on and
 * off; a line switched off shows its entry dimmed until it is brought back.
 * Returns what repaints the entries' pressed state.
 */
export function createLegend(container: HTMLElement, pane: Pane, shown: (metric: Metric) => boolean, toggle: (metric: Metric) => void): () => void {
  container.replaceChildren();
  pane.metrics.forEach((metric, position) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "item";
    item.dataset.metric = metric.key;
    item.title = "Show or hide this line";
    const key = document.createElement("span");
    key.className = "key";
    key.style.background = pane.colours[position];
    item.append(key, document.createTextNode(seriesName(metric)));
    item.addEventListener("click", () => toggle(metric));
    container.appendChild(item);
  });
  return () => {
    pane.metrics.forEach((metric, position) => {
      container.children[position]?.setAttribute("aria-pressed", String(shown(metric)));
    });
  };
}
