import { beforeEach, expect, test, vi } from "vitest";

// Stands in for uPlot, recording what the bar chart asks it to do.
const calls = { built: 0, setData: 0, setSize: 0 };
vi.mock("uplot", () => {
  class FakePlot {
    static pxRatio = 1;
    static paths = { bars: () => () => null };
    data: unknown[];
    over = { addEventListener: () => {} };
    cursor = { idx: null };
    constructor(_options: unknown, data: unknown[]) {
      calls.built += 1;
      this.data = data;
    }
    setData(data: unknown[]) {
      calls.setData += 1;
      this.data = data;
    }
    setSize() {
      calls.setSize += 1;
    }
    destroy() {}
  }
  return { default: FakePlot };
});

const { BarChart } = await import("./bars");

function chart() {
  const element = { style: {}, clientWidth: 600 } as unknown as HTMLElement;
  return new BarChart(element, { slotName: "Day", labelOf: String, unit: "kWh", decimals: 1, onPick: () => {} });
}

beforeEach(() => {
  calls.built = 0;
  calls.setData = 0;
  calls.setSize = 0;
});

test("the same bars given again are not drawn again", () => {
  const bars = chart();
  bars.setData([1, 2, 3], [4, 5, null], null, []);
  bars.setData([1, 2, 3], [4, 5, null], null, []);
  expect(calls.built).toBe(1);
  expect(calls.setData).toBe(0);
});

test("a bar that grew, a new outline or a new marker is drawn", () => {
  const bars = chart();
  bars.setData([1, 2, 3], [4, 5, null], null, []);
  bars.setData([1, 2, 3], [4, 5.1, null], null, []);
  bars.setData([1, 2, 3], [4, 5.1, null], { index: 1, low: 6, high: 7 }, []);
  bars.setData([1, 2, 3], [4, 5.1, null], { index: 1, low: 6, high: 7 }, [0]);
  expect(calls.setData).toBe(3);
});

test("the same size given again does not resize", () => {
  const bars = chart();
  bars.setData([1, 2, 3], [4, 5, null], null, []);
  bars.resize(600, 260);
  bars.resize(600, 260);
  expect(calls.setSize).toBe(1);
});
