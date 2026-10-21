import { expect, test } from "vitest";
import { alignWithProjection } from "./align";

test("readings and the projected path share one time axis, each empty where the other has a point", () => {
  const aligned = alignWithProjection([100, 200, 300], [[1, 2, 3]], {
    timestamps: [300, 600, 900],
    expected: [3, 5, 6],
    low: [3, 4, 5],
    high: [3, 6, 7],
  });
  expect(aligned.timestamps).toEqual([100, 200, 300, 600, 900]);
  expect(aligned.columns).toEqual([[1, 2, 3, null, null]]);
  expect(aligned.projected).toEqual([
    [null, null, 3, 5, 6],
    [null, null, 3, 4, 5],
    [null, null, 3, 6, 7],
  ]);
});
