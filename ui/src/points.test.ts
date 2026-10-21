import { expect, test } from "vitest";
import { pointsFor } from "./points";

test("a coarse pointer asks for two points per pixel and a fine one for everything", () => {
  expect(pointsFor(350, true)).toBe(700);
  expect(pointsFor(50, true)).toBe(200);
  expect(pointsFor(1200, false)).toBeUndefined();
});
