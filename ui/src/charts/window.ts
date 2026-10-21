export interface WindowInput {
  timestamps: number[];
  producing: Array<number | null> | null;
  isToday: boolean;
  dayStart: number;
  nowSeconds: number;
  /** Where today's projected path ends, when there is one. */
  projectionEnd: number | null;
}

/**
 * The stretch of a day worth reading: from the first reading that produced
 * power to the latest reading, or on a past day to the last one that
 * produced. With a projection, today runs on to where the projection ends.
 * A day with no generation yet falls back to its readings, and an empty one
 * to the whole day.
 */
export function defaultWindow(input: WindowInput): { min: number; max: number } {
  const { timestamps, producing, isToday, dayStart, nowSeconds, projectionEnd } = input;
  if (!timestamps.length) {
    return { min: dayStart, max: isToday ? Math.max(nowSeconds, dayStart + 60) : dayStart + 86_399 };
  }
  let firstProducing: number | null = null;
  let lastProducing: number | null = null;
  timestamps.forEach((timestamp, index) => {
    if ((producing?.[index] ?? 0) > 0) {
      if (firstProducing === null) firstProducing = timestamp;
      lastProducing = timestamp;
    }
  });
  const latest = timestamps[timestamps.length - 1];
  const min = firstProducing ?? timestamps[0];
  const end = isToday ? Math.max(latest, projectionEnd ?? latest) : (lastProducing ?? latest);
  return { min, max: Math.max(end, min + 60) };
}
