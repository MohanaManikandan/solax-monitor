import type { Metric, Now } from "../api/bindings";
import type { Context } from "../app/context";
import { SECTION_TITLES } from "../charts/panes";
import { formatDuration, formatNumber } from "../format";

const SECTION_ORDER = ["solar", "grid", "inverter"];

// Not registry driven, and deliberately so: these describe the device and
// the store rather than a reading.
const SYSTEM_ROWS: Array<[string, string]> = [
  ["inverter_serial", "Inverter"],
  ["firmware", "Firmware"],
  ["api_type", "API type"],
  ["poll_interval", "Polling every"],
];

/** One row of a card: the live value and, for a metric with a peak, today's
 * peak in gold after it, reading as "240.0 V / 260.0 V". */
function statRow(metric: Metric): HTMLElement {
  const row = document.createElement("div");
  row.className = "stat";
  row.dataset.metric = metric.key;
  const label = document.createElement("span");
  label.className = "name";
  label.textContent = metric.label;
  if (metric.note) {
    const note = document.createElement("span");
    note.className = "note";
    note.textContent = metric.note;
    label.appendChild(note);
  }
  const unit = metric.unit ? '<span class="unit">' + metric.unit + "</span>" : "";
  const reading = document.createElement("span");
  reading.className = "reading";
  reading.innerHTML =
    '<span class="live">–' + unit + "</span>" + (metric.peak ? '<span class="sep">/</span><span class="top">–' + unit + "</span>" : "");
  row.append(label, reading);
  return row;
}

/** Today's heat derating as the Inverter card shows it, or null when the
 * metrics that show it are not named. */
export function derateReading(count: number | null, seconds: number | null): string | null {
  // A server that predates the figure leaves it out, so undefined too.
  if (count == null) return null;
  if (count === 0) return "None";
  return `${count} ${count === 1 ? "time" : "times"}, ${formatDuration(seconds ?? 0)}`;
}

function sectionCard(name: string, title: string): HTMLElement {
  const card = document.createElement("section");
  card.className = "card";
  const heading = document.createElement("h2");
  heading.className = "section-title " + name;
  heading.textContent = title;
  card.appendChild(heading);
  return card;
}

export interface Cards {
  readonly element: HTMLElement;
  paint(now: Now | null): void;
}

/** The Solar, Grid and Inverter cards, a row per metric not already on the
 * hero card, and the System card. Live values, today's peaks in gold, and
 * the run mode row tinted with the inverter's own verdict. */
export function createCards(context: Context): Cards {
  const element = document.createElement("section");
  element.className = "sections loading";

  const bySection = new Map<string, Metric[]>();
  for (const metric of context.registry.metrics) {
    // Featured metrics ride on the hero card, and so does the grid
    // countdown, which reads zero most of the day.
    if (metric.feature || metric.key === context.gridCountdown?.key) continue;
    if (!bySection.has(metric.section)) bySection.set(metric.section, []);
    bySection.get(metric.section)!.push(metric);
  }
  const rows = new Map<string, HTMLElement>();
  const names = [...bySection.keys()].sort((first, second) => (SECTION_ORDER.indexOf(first) + 1 || 99) - (SECTION_ORDER.indexOf(second) + 1 || 99));
  // Today's heat derating sits with the inverter's own readings. The
  // inverter never reports it; the server reads it from them.
  const derateRow = document.createElement("div");
  derateRow.className = "stat";
  derateRow.dataset.derived = "heat_derating";
  derateRow.hidden = true;
  derateRow.innerHTML = '<span class="name">Heat derating today</span><span class="reading"><span class="live">–</span></span>';
  const derateValue = derateRow.querySelector(".live") as HTMLElement;
  for (const name of names) {
    const card = sectionCard(name, SECTION_TITLES[name] ?? name);
    for (const metric of bySection.get(name)!) {
      const row = statRow(metric);
      rows.set(metric.key, row);
      card.appendChild(row);
    }
    if (name === "inverter") card.appendChild(derateRow);
    element.appendChild(card);
  }
  const systemCard = sectionCard("system", "System");
  systemCard.id = "system-card";
  const systemValues = new Map<string, HTMLElement>();
  for (const [key, label] of SYSTEM_ROWS) {
    const row = document.createElement("div");
    row.className = "stat";
    row.dataset.system = key;
    row.innerHTML = '<span class="name"></span><span class="reading"><span class="live">–</span></span>';
    row.querySelector(".name")!.textContent = label;
    systemValues.set(key, row.querySelector(".live") as HTMLElement);
    systemCard.appendChild(row);
  }
  element.appendChild(systemCard);
  systemValues.get("inverter_serial")!.textContent = context.registry.site.inverter_serial || "unknown";
  systemValues.get("poll_interval")!.textContent = context.intervalSeconds + " s";

  return {
    element,
    paint(now) {
      const reading = now?.reading;
      if (!now || !reading) return;
      element.classList.remove("loading");
      for (const metric of context.registry.metrics) {
        const row = rows.get(metric.key);
        if (!row) continue;
        (row.querySelector(".reading .live")!.firstChild as Text).nodeValue = formatNumber(reading.values[metric.key], metric.decimals);
        const top = row.querySelector(".reading .top");
        // A null peak is painted too, as a dash, so a new day never shows
        // the previous day's figure.
        if (top && metric.key in now.peaks) (top.firstChild as Text).nodeValue = formatNumber(now.peaks[metric.key], metric.decimals);
      }
      const derating = derateReading(now.today.derate_count, now.today.derate_seconds);
      derateRow.hidden = derating === null;
      derateValue.textContent = derating ?? "–";
      const runMode = rows.get(context.runMode.key);
      const severity = reading.health.severity;
      runMode?.classList.toggle("fault", severity === "fault");
      runMode?.classList.toggle("warn", severity === "warn");
      systemValues.get("firmware")!.textContent = reading.device.ver ?? "–";
      systemValues.get("api_type")!.textContent = reading.device.inverter_type === null ? "–" : String(reading.device.inverter_type);
    },
  };
}
