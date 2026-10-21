import type { Curve, Day, DaySummary, LiveEvent, Now, Registry } from "./bindings";
import type { LiveHandlers, Transport } from "./transport";

/** The browser's way in: the page's own origin, signed in by its cookie. */
export class BrowserTransport implements Transport {
  constructor(private readonly base = "/api/v1") {}

  private async get<Body>(path: string): Promise<Body> {
    const response = await fetch(this.base + path, { credentials: "same-origin" });
    if (!response.ok) throw new Error(`${path} answered ${response.status}`);
    return (await response.json()) as Body;
  }

  registry() { return this.get<Registry>("/registry"); }
  now() { return this.get<Now>("/now"); }
  curve() { return this.get<Curve>("/curve"); }

  day(date: string, points?: number) {
    const query = new URLSearchParams({ date });
    if (points !== undefined) query.set("points", String(points));
    return this.get<Day>("/day?" + query);
  }

  days(from: string, to: string) {
    return this.get<DaySummary[]>("/days?" + new URLSearchParams({ from, to }));
  }

  openLive(handlers: LiveHandlers): () => void {
    const source = new EventSource(this.base + "/live", { withCredentials: true });
    source.onmessage = (message) => handlers.event(JSON.parse(message.data) as LiveEvent);
    source.onopen = () => handlers.opened();
    source.onerror = () => handlers.failed(source.readyState === EventSource.CLOSED);
    return () => source.close();
  }

  exportUrl(date: string) {
    return this.base + "/export?" + new URLSearchParams({ date });
  }
}
