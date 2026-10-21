import type { Day, LiveEvent, Now } from "../api/bindings";
import { DayData } from "../data/day-data";
import { LiveConnection } from "../data/live";
import { secondsUntilMidnight } from "../data/today";
import { localDate } from "../format";
import { addDays } from "../periods";
import { pointsFor } from "../points";
import type { Context } from "./context";
import type { AppState } from "./state";

/** Numbers a day load so an answer that lands after a newer request is
 * dropped: holding an arrow key asks faster than days arrive. */
let loadSequence = 0;

/** Past days kept for the visit, so stepping back and forth asks once. A
 * full-resolution day is a few megabytes, so only the latest few stay. */
const PAST_DAYS_KEPT = 14;

export class Controller {
  private readonly pastDays = new Map<string, Day>();

  constructor(private readonly context: Context, private readonly state: AppState, private readonly chartWidth: () => number) {}

  async boot(): Promise<void> {
    const { transport } = this.context;
    // Without the curve the page loses only the projection's dashed line.
    const [now, curve] = await Promise.all([
      transport.now(),
      transport.curve().catch((error) => {
        console.error("fetching the typical-day curve failed", error);
        return null;
      }),
    ]);
    // The server's day wins over the device clock the page started from.
    if (this.state.today.advanceTo(now.today.date)) this.state.selectedDate = this.state.today.today;
    this.applyNow(now);
    // Freshness is judged from when the latest stored reading was taken.
    if (now.reading) this.state.lastReadingMs = Math.min(now.reading.ts * 1000, Date.now());
    this.state.curve = curve;
    this.state.notify();
    await this.loadHistory();
    await this.selectDay(this.state.selectedDate);
    this.watchMidnight();
    new LiveConnection(transport, {
      event: (event) => this.applyEvent(event),
      reconnected: () => void this.catchUp(),
      trouble: () => {
        this.state.troubled = true;
        this.state.notify();
      },
    }).start();
  }

  /** The day rows behind the day arrows, Month and Year. A failure leaves
   * the rest of the page working, says why, and is tried again when the
   * stream reconnects. */
  private async loadHistory(): Promise<void> {
    const earliest = this.state.now?.earliest ?? null;
    const firstDay = earliest === null ? this.state.today.today : localDate(earliest, this.context.timezone);
    try {
      await this.state.days.load(firstDay, this.state.today.today);
      this.state.historyLoaded = true;
      this.state.historyError = null;
    } catch (error) {
      console.error("loading the day history failed", error);
      this.state.historyError = String(error);
    }
    this.state.notify();
  }

  async selectDay(date: string): Promise<void> {
    const sequence = ++loadSequence;
    this.state.selectedDate = date;
    this.state.loadError = null;
    this.state.notify();
    try {
      const day = this.pastDays.get(date) ?? (await this.fetchDay(date));
      if (sequence !== loadSequence) return;
      this.state.day = DayData.fromDay(day, this.context.chartKeys, this.context.intervalSeconds);
    } catch (error) {
      if (sequence !== loadSequence) return;
      this.state.loadError = String(error);
    }
    this.state.notify();
  }

  private async fetchDay(date: string): Promise<Day> {
    const coarse = matchMedia("(pointer: coarse)").matches;
    const day = await this.context.transport.day(date, pointsFor(this.chartWidth(), coarse));
    // Today keeps changing, so only days before it are kept.
    if (date < this.state.today.today) {
      this.pastDays.set(date, day);
      if (this.pastDays.size > PAST_DAYS_KEPT) this.pastDays.delete(this.pastDays.keys().next().value as string);
    }
    return day;
  }

  /** Takes in the server's present. Says whether it began a new local day,
   * in which case the page has already moved to it. */
  private applyNow(now: Now): boolean {
    const wasToday = this.state.isToday();
    this.state.now = now;
    this.state.sleeping = now.poller.sleeping;
    this.state.resumesAt = now.poller.resumes_at;
    this.state.days.upsert(now.today);
    if (!this.state.today.advanceTo(now.today.date)) return false;
    this.rollOver(wasToday);
    return true;
  }

  /** A new local day: a page showing today follows it to the new day's
   * chart, and a fresh curve is fetched for it. */
  private rollOver(wasToday: boolean): void {
    this.context.transport
      .curve()
      .then((curve) => {
        this.state.curve = curve;
        this.state.notify();
      })
      .catch((error) => console.error("fetching the new day's curve failed", error));
    if (wasToday) void this.selectDay(this.state.today.today);
  }

  /** Rolls the page over at the plant's midnight even when no reading
   * arrives to prompt it, as on a night the poller sleeps through. One
   * timer a day, set again each time it fires. */
  private watchMidnight(): void {
    const seconds = secondsUntilMidnight(Date.now() / 1000, this.context.timezone);
    setTimeout(() => {
      void this.atMidnight();
      this.watchMidnight();
    }, (seconds + 1) * 1000);
  }

  private async atMidnight(): Promise<void> {
    try {
      // The new day's present: its row starts empty.
      this.applyNow(await this.context.transport.now());
    } catch (error) {
      console.error("fetching the present at midnight failed", error);
    }
    // Should the server be unreachable or behind, the clock still moves the day.
    const wasToday = this.state.isToday();
    if (this.state.today.advanceTo(localDate(Date.now() / 1000, this.context.timezone))) this.rollOver(wasToday);
    this.state.notify();
  }

  private applyEvent(event: LiveEvent): void {
    if (event.type === "poller") {
      this.state.sleeping = event.sleeping;
      this.state.resumesAt = event.resumes_at;
      this.state.notify();
      return;
    }
    const reading = event.reading;
    // A reading from a new day begins it; the new day's chart is loaded
    // with that reading in it.
    const rolled = this.applyNow(event);
    if (reading) {
      if (!rolled && this.state.isToday() && this.state.day && this.state.day.date === this.state.today.today) {
        const values: Record<string, number | null> = {};
        for (const [key, value] of Object.entries(reading.values)) values[key] = typeof value === "number" ? value : null;
        this.state.day.append(reading.ts, values);
        if (event.stretches) this.state.day.setStretches(event.stretches);
      } else if (!rolled && this.state.isToday() && this.state.loadError !== null) {
        // Today's chart failed to load; a reading arriving shows the server
        // answering again, so it is tried once more, with this reading in it.
        void this.selectDay(this.state.today.today);
      }
      // Only a pushed reading proves the poller is awake right now.
      this.state.sleeping = false;
      this.state.troubled = false;
      this.state.lastReadingMs = Math.min(reading.ts * 1000, Date.now());
      this.state.readings += 1;
    }
    this.state.notify();
  }

  /** After the stream was closed: fetch the present again, and reload today
   * only if the store holds readings the chart never received. */
  private async catchUp(): Promise<void> {
    try {
      const now = await this.context.transport.now();
      const shownUntil = this.state.day?.latest() ?? 0;
      // A new day while hidden has already moved the page to it.
      const rolled = this.applyNow(now);
      this.state.troubled = false;
      if (!this.state.historyLoaded) await this.loadHistory();
      if (now.reading) this.state.lastReadingMs = Math.min(now.reading.ts * 1000, Date.now());
      const missed = now.reading !== null && now.reading.ts - shownUntil > this.context.intervalSeconds * 3;
      if (!rolled && this.state.isToday() && missed) await this.selectDay(this.state.today.today);
      else this.state.notify();
    } catch (error) {
      console.error("catching up failed", error);
    }
  }

  previousDay(): string | null {
    return this.state.days.earlier(this.state.selectedDate);
  }

  nextDay(): string | null {
    const later = this.state.days.later(this.state.selectedDate);
    return later ?? (this.state.selectedDate < this.state.today.today ? addDays(this.state.selectedDate, 1) : null);
  }
}
