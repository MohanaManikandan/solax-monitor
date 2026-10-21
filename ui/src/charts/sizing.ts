/** Below this width the page is a phone's single column. */
export const PHONE_WIDTH = 900;
/** From this width the readings and the charts sit side by side. */
export const WIDE_WIDTH = 1400;

export interface ChartLayout {
  viewportWidth: number;
  viewportHeight: number;
  /** On a wide screen, the natural height of the column beside the charts. */
  columnHeight: number;
  /** On a wide screen, the room from the top of the chart card down to the
   * footer. */
  screenSpace: number;
  /** Everything in the chart card that is not a canvas. */
  chrome: number;
}

/**
 * How tall the power chart and the chart under it are. A phone gives the
 * power chart about 45% of the screen. A narrow computer screen keeps the
 * old page's share of the window. A wide screen fills down to the footer,
 * or as far as the cards beside the charts reach when that is further, so
 * neither column leaves a gap; the power chart keeps the larger share.
 */
export function chartHeights(layout: ChartLayout): { main: number; companion: number } {
  const below = (main: number) => Math.max(200, Math.round(main * 0.6));
  if (layout.viewportWidth < PHONE_WIDTH) {
    const main = Math.max(240, Math.round(layout.viewportHeight * 0.45));
    return { main, companion: below(main) };
  }
  const narrow = Math.round(Math.min(560, Math.max(260, layout.viewportHeight * 0.42)));
  const fallback = { main: narrow, companion: below(narrow) };
  if (layout.viewportWidth < WIDE_WIDTH) return fallback;
  const available = Math.round(Math.max(layout.columnHeight, layout.screenSpace) - layout.chrome);
  if (available < 480) return fallback;
  const companion = Math.round(available * 0.4);
  return { main: available - companion, companion };
}

export interface BarsLayout {
  viewportWidth: number;
  viewportHeight: number;
  /** Where the bars start, from the top of the window. */
  barsTop: number;
  /** Room needed under the bars: their readout, the card's edge and the footer. */
  below: number;
  /** Room under the bars inside their own card: the readout and the card's edge. */
  inCard: number;
  /** Where the cards beside the bars end, measured as barsTop is. */
  columnBottom: number;
}

/** How tall a Month or Year bar chart is: fixed on a phone, and on a
 * computer down to the footer, or as far as the cards beside it reach when
 * that is further, never shorter than a readable chart. */
export function barsHeight(layout: BarsLayout): number {
  if (layout.viewportWidth < PHONE_WIDTH) return 200;
  const toFooter = layout.viewportHeight - layout.barsTop - layout.below;
  const toColumnEnd = layout.columnBottom - layout.barsTop - layout.inCard;
  return Math.max(260, Math.round(Math.max(toFooter, toColumnEnd)));
}
