import uPlot from "uplot";

export const SUN = "#ffc24d";
export const MAINS = "#4fd6c8";
export const EMBER = "#ff8f4c";
export const LILAC = "#b8a6ff";
export const FAINT = "#737373";
export const HAIRLINE = "#262626";
export const WELL = "#060606";

// A series wears its section's colour, so solar reads yellow and grid teal on
// every chart. A second series from the same section on one chart takes a
// spare instead.
export const SECTION_COLOURS: Record<string, string> = { solar: SUN, grid: MAINS, inverter: EMBER };
export const SPARE_COLOURS = [LILAC];

// The canvas is drawn in device pixels while everything here is reckoned in
// CSS pixels. At 180% zoom on a 4K panel, text drawn unscaled came out at a
// little over half its intended size.
export function pixelRatio(): number {
  return uPlot.pxRatio || window.devicePixelRatio || 1;
}

export function colour(hex: string, alpha: number): string {
  const value = parseInt(hex.slice(1), 16);
  return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, ${alpha})`;
}

// Axis ticks are reference furniture, not the point of the chart. They grow
// gently with the chart: readable on a big panel without crowding out the
// curve they are labelling.
export function axisTextSize(widthCssPixels: number): number {
  const width = widthCssPixels || 640;
  return Math.round(Math.min(15, Math.max(11, width / 90)));
}

export function axisFont(size: number): string {
  return size + "px Inter, system-ui, sans-serif";
}
