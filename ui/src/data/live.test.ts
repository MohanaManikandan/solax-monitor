import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { LiveHandlers } from "../api/transport";
import { LiveConnection, type Visibility } from "./live";

class FakeVisibility implements Visibility {
  shown = true;
  private listeners: Array<() => void> = [];
  visible() { return this.shown; }
  onChange(listener: () => void) { this.listeners.push(listener); }
  set(shown: boolean) { this.shown = shown; for (const listener of this.listeners) listener(); }
}

function harness() {
  const opened: LiveHandlers[] = [];
  let closed = 0;
  const transport = { openLive(handlers: LiveHandlers) { opened.push(handlers); return () => { closed += 1; }; } };
  const callbacks = { event: vi.fn(), reconnected: vi.fn(), trouble: vi.fn() };
  const visibility = new FakeVisibility();
  const connection = new LiveConnection(transport, callbacks, visibility);
  return { opened, closedCount: () => closed, callbacks, visibility, connection };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test("the stream closes while hidden and reconnects with a catch-up when shown", () => {
  const { opened, closedCount, callbacks, visibility, connection } = harness();
  connection.start();
  opened[0].opened();
  expect(callbacks.reconnected).not.toHaveBeenCalled();
  visibility.set(false);
  expect(closedCount()).toBe(1);
  visibility.set(true);
  expect(opened.length).toBe(2);
  opened[1].opened();
  expect(callbacks.reconnected).toHaveBeenCalledTimes(1);
});

test("a stream that gives up is retried with a growing wait, and not while hidden", () => {
  const { opened, callbacks, visibility, connection } = harness();
  connection.start();
  opened[0].failed(true);
  expect(callbacks.trouble).toHaveBeenCalled();
  vi.advanceTimersByTime(5000);
  expect(opened.length).toBe(2);
  opened[1].failed(true);
  vi.advanceTimersByTime(5000);
  expect(opened.length).toBe(2);
  vi.advanceTimersByTime(5000);
  expect(opened.length).toBe(3);
  opened[2].failed(true);
  visibility.set(false);
  vi.advanceTimersByTime(60_000);
  expect(opened.length).toBe(3);
});

test("a page that starts hidden opens nothing until it is shown", () => {
  const { opened, visibility, connection } = harness();
  visibility.shown = false;
  connection.start();
  expect(opened.length).toBe(0);
});
