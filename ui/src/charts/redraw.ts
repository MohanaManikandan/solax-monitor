/** What a chart last drew, as far as deciding how to draw next. */
export interface DrawnDay {
  date: string;
  /** Whether today's projection was drawn with it. */
  projected: boolean;
}

/** Whether a chart is built afresh, which drops any zoom, rather than given
 * the new readings. Only a different day, or the projection's series coming
 * or going, needs that; the same day loaded again, as when a hidden page
 * catches up, is more readings for it. */
export function rebuildNeeded(previous: DrawnDay | null, next: DrawnDay): boolean {
  return previous === null || previous.date !== next.date || previous.projected !== next.projected;
}
