import type { Curve, Now } from "../api/bindings";
import type { DayData } from "../data/day-data";
import type { DaysIndex } from "../data/days-index";
import type { TodayTracker } from "../data/today";

type Listener = () => void;

/** Everything that changes during a visit. Screens read it and repaint when
 * told; only the visible screen does any drawing. */
export class AppState {
  now: Now | null = null;
  curve: Curve | null = null;
  selectedDate: string;
  day: DayData | null = null;
  sleeping = false;
  resumesAt: number | null = null;
  lastReadingMs = 0;
  /** The live stream failed and has not delivered a reading since. */
  troubled = false;
  /** Readings pushed by the live stream this visit, so each one can be
   * marked once. */
  readings = 0;
  /** Why the selected day could not be loaded, until the next load. */
  loadError: string | null = null;
  /** The day rows have arrived, so an empty period really is empty. */
  historyLoaded = false;
  /** Why the day rows could not be loaded, until they are. */
  historyError: string | null = null;
  /** Lines the person switched on or off, by metric key, for the visit. */
  readonly toggles = new Map<string, boolean>();
  private readonly listeners = new Set<Listener>();

  constructor(readonly days: DaysIndex, readonly today: TodayTracker) {
    this.selectedDate = today.today;
  }

  isToday(): boolean {
    return this.selectedDate === this.today.today;
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  notify(): void {
    for (const listener of this.listeners) listener();
  }
}
