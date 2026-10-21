import { expect, test } from "vitest";
import { defaultWindow } from "./window";

const timestamps = [100, 200, 300, 400, 500];
const producing = [0, 5, 9, 4, 0];

test("a past day opens from its first to its last producing reading", () => {
  expect(defaultWindow({ timestamps, producing, isToday: false, dayStart: 0, nowSeconds: 9999, projectionEnd: null })).toEqual({ min: 200, max: 400 });
});

test("today runs to the latest reading, or to the end of the projection when there is one", () => {
  expect(defaultWindow({ timestamps, producing, isToday: true, dayStart: 0, nowSeconds: 9999, projectionEnd: null })).toEqual({ min: 200, max: 500 });
  expect(defaultWindow({ timestamps, producing, isToday: true, dayStart: 0, nowSeconds: 9999, projectionEnd: 900 })).toEqual({ min: 200, max: 900 });
});

test("an empty day still gives a window", () => {
  expect(defaultWindow({ timestamps: [], producing: null, isToday: false, dayStart: 1000, nowSeconds: 9999, projectionEnd: null })).toEqual({ min: 1000, max: 87_399 });
});
