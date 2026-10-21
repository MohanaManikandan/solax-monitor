import { expect, test } from "vitest";
import { ChartGroup, type GroupMember } from "./group";

function member(log: string[], name: string): GroupMember {
  return {
    followWindow: (min, max) => log.push(`${name} follows ${min}-${max}`),
    resetWindow: () => log.push(`${name} resets`),
  };
}

test("a window one chart moves to is followed by the others, not by itself", () => {
  const log: string[] = [];
  const group = new ChartGroup();
  const power = member(log, "power");
  const voltage = member(log, "voltage");
  group.join(power);
  group.join(voltage);
  group.moved(power, 100, 200);
  expect(log).toEqual(["voltage follows 100-200"]);
});

test("a reset drops the zoom and puts every chart back on its opening window", () => {
  const log: string[] = [];
  const group = new ChartGroup();
  group.join(member(log, "power"));
  group.join(member(log, "voltage"));
  group.zoomed = true;
  group.followsLive = true;
  group.reset();
  expect(group.zoomed).toBe(false);
  expect(group.followsLive).toBe(false);
  expect(log).toEqual(["power resets", "voltage resets"]);
});

test("a chart that left the group is no longer moved", () => {
  const log: string[] = [];
  const group = new ChartGroup();
  const power = member(log, "power");
  const voltage = member(log, "voltage");
  group.join(power);
  group.join(voltage);
  group.leave(voltage);
  group.moved(power, 1, 2);
  expect(log).toEqual([]);
});
