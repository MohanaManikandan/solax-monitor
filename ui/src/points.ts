const FEWEST_POINTS = 200;

/** How many points a day chart asks for: two per pixel on a touch screen,
 * which cannot zoom, and every reading on a mouse-driven one, which can. */
export function pointsFor(widthCssPixels: number, coarsePointer: boolean): number | undefined {
  if (!coarsePointer) return undefined;
  return Math.max(FEWEST_POINTS, Math.round(widthCssPixels * 2));
}
