import { expect, test } from "vitest";
import { projectionLine } from "./projection-text";

test("each state reads as the spec words it", () => {
  expect(projectionLine({ state: "not_enough_history", days: 2 }, 1, "kWh")).toBe("Projection after 3 full days");
  expect(projectionLine({ state: "too_early", from: "08:30" }, 1, "kWh")).toBe("Projection from about 08:30");
  expect(
    projectionLine({ state: "on_pace", expected_kwh: 15, low_kwh: 13.6, high_kwh: 16.7, based_on_days: 14 }, 1, "kWh", "en-US"),
  ).toBe("On pace for 15.0 kWh · 13.6 to 16.7");
  expect(projectionLine({ state: "finished", yield_kwh: 19.5 }, 1, "kWh")).toBeNull();
});

test("a too early projection without a time hides the line", () => {
  expect(projectionLine({ state: "too_early", from: null }, 1, "kWh")).toBeNull();
});
