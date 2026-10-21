import { expect, test } from "vitest";
import { joinClose, visibleSpans } from "./stretches";

test("stretches a short dip apart are drawn as one, unless a derate sits in the dip", () => {
  const atLimit: Array<[number, number]> = [[0, 100], [150, 300], [500, 600], [700, 800], [860, 900]];
  const derating: Array<[number, number]> = [[810, 850]];
  expect(joinClose(atLimit, 120, derating)).toEqual([[0, 300], [500, 800], [860, 900]]);
});

test("a stretch is drawn across the part of it inside the plot, and one outside is left out", () => {
  // One second per pixel, with the plot from 100 to 500.
  const xOf = (ts: number) => ts;
  expect(visibleSpans([[50, 150], [200, 300], [450, 900], [600, 700]], xOf, 100, 500)).toEqual([
    [100, 150],
    [200, 300],
    [450, 500],
  ]);
});

test("a stretch narrower than a pixel still shows as one", () => {
  expect(visibleSpans([[200, 200.2]], (ts) => ts, 100, 500)).toEqual([[200, 201]]);
});
