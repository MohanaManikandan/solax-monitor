function toUtc(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  return Date.UTC(year, month - 1, day);
}

function fromUtc(milliseconds: number): string {
  return new Date(milliseconds).toISOString().slice(0, 10);
}

export function addDays(date: string, days: number): string {
  return fromUtc(toUtc(date) + days * 86_400_000);
}

export function monthOf(date: string): string {
  return date.slice(0, 7);
}

function monthIndex(month: string): number {
  const [year, monthNumber] = month.split("-").map(Number);
  return year * 12 + (monthNumber - 1);
}

function monthFromIndex(index: number): string {
  const year = Math.floor(index / 12);
  return `${year}-${String((index % 12) + 1).padStart(2, "0")}`;
}

export function addMonths(month: string, months: number): string {
  return monthFromIndex(monthIndex(month) + months);
}

export function daysInMonth(month: string): string[] {
  const days: string[] = [];
  for (let date = month + "-01"; monthOf(date) === month; date = addDays(date, 1)) days.push(date);
  return days;
}

/** Consecutive ranges covering `from` to `to`, none longer than `maxDays`. */
export function splitRange(from: string, to: string, maxDays = 400): Array<[string, string]> {
  const ranges: Array<[string, string]> = [];
  for (let start = from; start <= to; start = addDays(start, maxDays)) {
    const end = addDays(start, maxDays - 1);
    ranges.push([start, end < to ? end : to]);
  }
  return ranges;
}

function nearest(candidates: string[], target: string, toIndex: (value: string) => number): string | null {
  let best: string | null = null;
  let bestDistance = Infinity;
  for (const candidate of [...candidates].sort()) {
    const distance = Math.abs(toIndex(candidate) - toIndex(target));
    if (distance < bestDistance) {
      best = candidate;
      bestDistance = distance;
    }
  }
  return best;
}

export function nearestMonth(months: string[], target: string): string | null {
  return nearest(months, target, monthIndex);
}

export function nearestYear(years: string[], target: string): string | null {
  return nearest(years, target, Number);
}

export function monthTitle(month: string, locale?: string): string {
  return new Date(toUtc(month + "-01")).toLocaleDateString(locale, { month: "long", year: "numeric", timeZone: "UTC" });
}

export function monthName(month: string, locale?: string): string {
  return new Date(toUtc(month + "-01")).toLocaleDateString(locale, { month: "long", timeZone: "UTC" });
}
