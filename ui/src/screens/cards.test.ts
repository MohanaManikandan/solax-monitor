import { expect, test } from "vitest";
import { derateReading } from "./cards";

/** The text with no-break spaces, which keep it on one line. */
const unbroken = (text: string) => text.replaceAll(" ", "\u00a0");

test("today's heat derating reads as how many times and for how long, or none", () => {
  expect(derateReading(4, 2220)).toBe(`4 times, ${unbroken("37 min")}`);
  expect(derateReading(1, 540)).toBe(`1 time, ${unbroken("9 min")}`);
  expect(derateReading(0, 0)).toBe("None");
  expect(derateReading(null, null)).toBeNull();
  // A server that predates the figure leaves it out altogether.
  expect(derateReading(undefined as unknown as null, undefined as unknown as null)).toBeNull();
});
