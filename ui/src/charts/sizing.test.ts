import { describe, expect, test } from "vitest";
import { barsHeight, chartHeights } from "./sizing";

describe("chartHeights", () => {
  test("a phone gives the power chart about 45% of the screen and the second chart 60% of that", () => {
    expect(chartHeights({ viewportWidth: 390, viewportHeight: 844, columnHeight: 0, screenSpace: 0, chrome: 0 })).toEqual({ main: 380, companion: 228 });
  });

  test("a narrow computer screen keeps the old page's share of the window", () => {
    expect(chartHeights({ viewportWidth: 1280, viewportHeight: 1000, columnHeight: 0, screenSpace: 0, chrome: 0 })).toEqual({ main: 420, companion: 252 });
  });

  test("a wide screen fills down to the footer when the cards beside the charts are shorter", () => {
    const heights = chartHeights({ viewportWidth: 2560, viewportHeight: 1440, columnHeight: 900, screenSpace: 1300, chrome: 200 });
    expect(heights.main + heights.companion).toBe(1100);
    expect(heights.companion).toBe(440);
  });

  test("a wide screen matches the column of cards when that runs further down than the screen", () => {
    const heights = chartHeights({ viewportWidth: 1920, viewportHeight: 1080, columnHeight: 1400, screenSpace: 900, chrome: 200 });
    expect(heights.main + heights.companion).toBe(1200);
  });

  test("a wide but short window falls back to the narrow rule rather than squashing the charts", () => {
    expect(chartHeights({ viewportWidth: 1600, viewportHeight: 600, columnHeight: 400, screenSpace: 500, chrome: 200 })).toEqual({ main: 260, companion: 200 });
  });
});

describe("barsHeight", () => {
  const beside = { inCard: 50, columnBottom: 0 };

  test("a phone keeps a fixed height", () => {
    expect(barsHeight({ viewportWidth: 390, viewportHeight: 844, barsTop: 300, below: 120, ...beside, columnBottom: 1400 })).toBe(200);
  });

  test("a computer screen grows the bars down to the footer", () => {
    expect(barsHeight({ viewportWidth: 1920, viewportHeight: 1080, barsTop: 260, below: 120, ...beside })).toBe(700);
  });

  test("the bars reach as far as the cards beside them when those run past the window", () => {
    expect(barsHeight({ viewportWidth: 1366, viewportHeight: 768, barsTop: 150, below: 120, inCard: 50, columnBottom: 900 })).toBe(700);
  });

  test("never shorter than a readable chart", () => {
    expect(barsHeight({ viewportWidth: 1280, viewportHeight: 500, barsTop: 260, below: 120, ...beside })).toBe(260);
  });
});
