import { expect, test } from "vitest";
import { valueParts } from "./period";

test("a tile's value sets its numbers apart from the words around them", () => {
  expect(valueParts("2,081.1 kWh")).toEqual([
    { text: "2,081.1", unit: false },
    { text: " kWh", unit: true },
  ]);
  expect(valueParts("5 h 30 min")).toEqual([
    { text: "5", unit: false },
    { text: " h ", unit: true },
    { text: "30", unit: false },
    { text: " min", unit: true },
  ]);
  expect(valueParts("30")).toEqual([{ text: "30", unit: false }]);
});
