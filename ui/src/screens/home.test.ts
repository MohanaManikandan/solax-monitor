import { expect, test } from "vitest";
import { heroLines, meterCaption } from "./home";

test("the meter's caption counts down to the grid connection, then measures against the real limit", () => {
  expect(meterCaption(0, 3300, 37)).toBe("Connecting to the grid in 37 s");
  expect(meterCaption(20, 3300, 0)).toBe("Idle");
  expect(meterCaption(1650, 3300, null)).toBe("50% of the inverter's limit");
  expect(meterCaption(3250, 3300, 0)).toBe("At the inverter's limit");
});

test("the hero card's lines come from today's figures", () => {
  const lines = heroLines(
    { yield_kwh: 6, peak_power_w: 3447, peak_at: Date.UTC(2026, 9, 4, 5, 11) / 1000 },
    { state: "on_pace", expected_kwh: 15, low_kwh: 13.6, high_kwh: 16.7, based_on_days: 14 },
    { decimals: 1, unit: "kWh", heroUnit: "W", timezone: "Asia/Kolkata", locale: "en-GB" },
  );
  expect(lines.projection).toBe("On pace for 15.0 kWh · 13.6 to 16.7");
  expect(lines.peak).toBe("Peak today 3,447 W at 10:41");
});

test("no peak yet hides the peak line", () => {
  const lines = heroLines(
    { yield_kwh: null, peak_power_w: null, peak_at: null },
    { state: "not_enough_history", days: 0 },
    { decimals: 1, unit: "kWh", heroUnit: "W", timezone: "Asia/Kolkata", locale: "en-GB" },
  );
  expect(lines.peak).toBeNull();
  expect(lines.projection).toBe("Projection after 3 full days");
});
