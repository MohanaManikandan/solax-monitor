import type { Curve, Day, DaySummary, LiveEvent, Now, Registry } from "./bindings";

export interface LiveHandlers {
  event(event: LiveEvent): void;
  opened(): void;
  /** `gaveUp` when the stream will not retry by itself. */
  failed(gaveUp: boolean): void;
}

/** Where the screens get their data. The browser asks the server directly;
 * the app asks its own core. */
export interface Transport {
  registry(): Promise<Registry>;
  now(): Promise<Now>;
  curve(): Promise<Curve>;
  day(date: string, points?: number): Promise<Day>;
  days(from: string, to: string): Promise<DaySummary[]>;
  /** Opens the live stream and returns what closes it. */
  openLive(handlers: LiveHandlers): () => void;
  exportUrl(date: string, refresh?: boolean): string;
}
