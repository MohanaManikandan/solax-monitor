import type { LiveEvent } from "../api/bindings";
import type { Transport } from "../api/transport";

export interface Visibility {
  visible(): boolean;
  onChange(listener: () => void): void;
}

export const browserVisibility: Visibility = {
  visible: () => document.visibilityState === "visible",
  onChange: (listener) => document.addEventListener("visibilitychange", listener),
};

export interface LiveCallbacks {
  event(event: LiveEvent): void;
  /** The stream opened again after being closed, so readings may be missing. */
  reconnected(): void;
  trouble(): void;
}

const FIRST_RETRY_MS = 5_000;
const LONGEST_RETRY_MS = 60_000;

/** The live stream, open only while the page can be seen. A stream that
 * gives up is reopened after 5 seconds, backing off to once a minute. */
export class LiveConnection {
  private close: (() => void) | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retryDelay = FIRST_RETRY_MS;
  private openedBefore = false;

  constructor(
    private readonly transport: Pick<Transport, "openLive">,
    private readonly callbacks: LiveCallbacks,
    private readonly visibility: Visibility = browserVisibility,
  ) {}

  start(): void {
    this.visibility.onChange(() => (this.visibility.visible() ? this.open() : this.shut()));
    if (this.visibility.visible()) this.open();
  }

  private open(): void {
    this.shut();
    this.close = this.transport.openLive({
      event: (event) => this.callbacks.event(event),
      opened: () => {
        this.retryDelay = FIRST_RETRY_MS;
        if (this.openedBefore) this.callbacks.reconnected();
        this.openedBefore = true;
      },
      failed: (gaveUp) => {
        this.callbacks.trouble();
        if (gaveUp) this.scheduleRetry();
      },
    });
  }

  private shut(): void {
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.close?.();
    this.close = null;
  }

  private scheduleRetry(): void {
    if (this.retryTimer !== null || !this.visibility.visible()) return;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.open();
    }, this.retryDelay);
    this.retryDelay = Math.min(this.retryDelay * 2, LONGEST_RETRY_MS);
  }
}
