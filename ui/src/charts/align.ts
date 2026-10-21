import type { ProjectionPath } from "../projection-path";

type Column = Array<number | null>;

export interface Aligned {
  timestamps: number[];
  columns: Column[];
  /** The expected, low and high paths, in that order. */
  projected: Column[];
}

/** One shared time axis for the day's readings and the projected path. */
export function alignWithProjection(timestamps: number[], columns: Column[], path: ProjectionPath): Aligned {
  const merged = [...new Set([...timestamps, ...path.timestamps])].sort((first, second) => first - second);
  const readingAt = new Map(timestamps.map((timestamp, index) => [timestamp, index]));
  const pathAt = new Map(path.timestamps.map((timestamp, index) => [timestamp, index]));
  const pick = (values: Column, positions: Map<number, number>) =>
    merged.map((timestamp) => {
      const position = positions.get(timestamp);
      return position === undefined ? null : values[position];
    });
  return {
    timestamps: merged,
    columns: columns.map((column) => pick(column, readingAt)),
    projected: [pick(path.expected, pathAt), pick(path.low, pathAt), pick(path.high, pathAt)],
  };
}
