import { expect, test } from "vitest";
import { addressOf, parseRoute, TABS } from "./router";

test("addresses round-trip, and anything unknown is Home", () => {
  expect(parseRoute("#/month/2026-09")).toEqual({ tab: "month", param: "2026-09" });
  expect(parseRoute("#/year/2026")).toEqual({ tab: "year", param: "2026" });
  expect(parseRoute("#/elsewhere")).toEqual({ tab: "home", param: null });
  expect(addressOf({ tab: "month", param: null }, "2026-10-04")).toBe("#/month/2026-10");
});

test("the old Details address opens Home, which now holds everything Details had", () => {
  expect(parseRoute("#/details")).toEqual({ tab: "home", param: null });
  expect(TABS).toEqual(["home", "month", "year"]);
});
