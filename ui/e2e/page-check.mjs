// Visits every tab at every screen size against a running server, records a
// full-page screenshot and the page's text for each, and checks the layout
// rules, reporting every problem and console error. Home is recorded twice:
// first on today, as the page opens, then on the given past day. A tab the
// page does not have is skipped.
//
//   node e2e/page-check.mjs <base-url> <out-prefix> [date]
import fs from "node:fs";
import { chromium } from "playwright";

const [baseUrl, outputPrefix, date = "2026-10-03"] = process.argv.slice(2);
const tabs = [
  ["today", "home", "#/"],
  ["home", "home", "#/"],
  ["month", "month", "#/month/" + date.slice(0, 7)],
  ["year", "year", "#/year/" + date.slice(0, 4)],
];
// Name, viewport in CSS pixels, touch, device pixels per CSS pixel.
const viewports = [
  ["phone", { width: 390, height: 844 }, true, 3],
  ["desktop", { width: 1280, height: 1000 }, false, 1],
  // A short laptop screen, where the tiles beside the bars outrun the window.
  ["laptop", { width: 1366, height: 768 }, false, 1],
  // At 1400 px and wider the screens lay out in two columns.
  ["wide", { width: 1440, height: 1000 }, false, 1],
  ["large", { width: 1920, height: 1080 }, false, 1],
  // A 4K panel with the browser zoomed to 220%.
  ["zoomed", { width: 1745, height: 982 }, false, 2.2],
];
const summary = { problems: [], pages: {}, charts: {} };

const browser = await chromium.launch();
for (const [viewportName, viewport, touch, deviceScaleFactor] of viewports) {
  for (const [captureName, tab, address] of tabs) {
    // A fresh page for every capture: a full-page screenshot drops the touch
    // emulation, so a page reused after one would report a mouse pointer.
    const page = await browser.newPage({ viewport, hasTouch: touch, isMobile: touch, deviceScaleFactor });
    page.on("console", (message) => {
      if (message.type() === "error") summary.problems.push(`${viewportName}: ${message.text()}`);
    });
    page.on("pageerror", (error) => summary.problems.push(`${viewportName}: ${error}`));
    await page.goto(baseUrl + "/?token=change-me" + address);
    await page.waitForLoadState("networkidle");
    const exists = await page.evaluate((name) => document.querySelector(`nav.tabs a[data-tab="${name}"]`) !== null, tab);
    if (!exists) {
      await page.close();
      continue;
    }
    if (captureName !== "today" && (tab === "home" || tab === "details")) {
      await page.evaluate((day) => window.dispatchEvent(new CustomEvent("solax:select-day", { detail: day })), date);
    }
    await page.waitForTimeout(1500);
    const key = `${captureName}-${viewportName}`;
    // Measured before the screenshot, which drops the touch emulation: every
    // visible control on a touch screen is at least 44 px square.
    if (touch) {
      const small = await page.evaluate(() =>
        [...document.querySelectorAll("button, a, input, select")]
          .filter((element) => element.offsetParent !== null)
          .map((element) => {
            const box = element.getBoundingClientRect();
            const name = (element.textContent || element.getAttribute("aria-label") || element.className).trim().slice(0, 30);
            return { name, width: Math.round(box.width), height: Math.round(box.height) };
          })
          .filter((target) => target.width < 44 || target.height < 44),
      );
      for (const target of small) summary.problems.push(`${key}: "${target.name}" is ${target.width} by ${target.height} px`);
    }
    // Month and Year are a summary card, a bars card and the figures as
    // tiles. On a computer the tiles sit under the summary and the bars
    // stand beside both, reaching at least as far down as the last tile; on
    // a phone it is the summary, the bars, then the tiles. An empty period
    // shows only its summary, across the screen.
    const period = await page.evaluate(() => {
      const screen = document.querySelector(".screen:not([hidden])");
      if (!screen || !["month", "year"].includes(screen.dataset.screen)) return null;
      const summaryCard = screen.querySelector(":scope > .summary-card");
      const barsCard = screen.querySelector(":scope > .bars-card");
      const tiles = screen.querySelector(":scope > .tiles");
      if (!summaryCard || !barsCard || !tiles) return "a period needs its summary card, its bars card and its tiles";
      const left = summaryCard.getBoundingClientRect();
      const whole = screen.getBoundingClientRect();
      if (barsCard.hidden) {
        if (!tiles.hidden) return "an empty period still shows its tiles";
        return left.width < whole.width - 1 ? "an empty period's summary does not span the screen" : null;
      }
      if (!tiles.querySelector(".tile")) return "a period with readings shows no tiles";
      const right = barsCard.getBoundingClientRect();
      const under = tiles.getBoundingClientRect();
      if (window.innerWidth >= 900) {
        if (left.right > right.left || under.right > right.left) return "the summary or the tiles overlap the bars";
        if (Math.abs(left.top - right.top) > 1) return "the summary and the bars do not start level";
        if (under.top < left.bottom) return "the tiles do not sit under the summary";
        if (right.right < whole.right - 1) return "the bars stop short of the screen's edge";
        if (right.bottom < under.bottom - 1) return `the bars end ${Math.round(under.bottom - right.bottom)} px above the last tile`;
      } else {
        if (left.bottom > right.top + 1) return "on a phone the summary comes before the bars";
        if (right.bottom > under.top + 1) return "on a phone the bars come before the tiles";
      }
      return null;
    });
    if (period) summary.problems.push(`${key}: ${period}`);
    // The charts on Home share one time window. With the Generated line off,
    // its projection is hidden and must not stretch that window past the
    // latest reading.
    const windows = await page.evaluate(() => {
      const found = [];
      const plots = [...document.querySelectorAll(".screen:not([hidden]) [data-chart]")].map((element) => element.solaxPlot).filter(Boolean);
      const ranges = plots.map((plot) => [plot.scales.x.min, plot.scales.x.max]);
      if (ranges.some(([min, max]) => Math.abs(min - ranges[0][0]) > 1 || Math.abs(max - ranges[0][1]) > 1)) {
        found.push("the charts show different time windows: " + ranges.map(([min, max]) => `${Math.round(min)}-${Math.round(max)}`).join(", "));
      }
      const generated = document.querySelector('.main-legend .item[data-metric="today_kwh"]');
      const power = plots[0];
      if (power && generated && generated.getAttribute("aria-pressed") === "false") {
        const timestamps = power.data[0];
        const hero = power.data[1];
        let latest = null;
        for (let index = 0; index < timestamps.length; index++) if (hero[index] !== null && hero[index] !== undefined) latest = timestamps[index];
        if (latest !== null && power.scales.x.max > latest + 60) {
          found.push(`with Generated off the power chart runs ${Math.round((power.scales.x.max - latest) / 60)} min past the latest reading`);
        }
      }
      return found;
    });
    for (const problem of windows) summary.problems.push(`${key}: ${problem}`);
    // On a computer the page fills the window, and every chart stays inside
    // its card. On Home's two columns, neither column ends short of the other.
    const gaps = await page.evaluate(() => {
      const found = [];
      const page = document.querySelector(".page").getBoundingClientRect();
      if (window.innerWidth >= 900) {
        if (page.width < window.innerWidth - 60) found.push(`the page is ${Math.round(page.width)} of ${window.innerWidth} px wide`);
        const height = document.documentElement.scrollHeight;
        if (height < window.innerHeight - 2) found.push(`the page ends at ${height} of ${window.innerHeight} px`);
      }
      for (const canvas of document.querySelectorAll(".screen:not([hidden]) canvas")) {
        const card = canvas.closest(".card");
        if (!card) continue;
        const inner = canvas.getBoundingClientRect();
        const outer = card.getBoundingClientRect();
        if (inner.right > outer.right + 1 || inner.left < outer.left - 1) {
          found.push(`a chart runs past its card (${Math.round(inner.left)}-${Math.round(inner.right)} in ${Math.round(outer.left)}-${Math.round(outer.right)})`);
        }
      }
      const charts = document.querySelector(".screen:not([hidden]) > .chart-card");
      const cards = document.querySelector(".screen:not([hidden]) > .sections");
      if (window.innerWidth >= 1400 && charts && cards) {
        // The cards stay as tall as their rows; the charts reach at least as
        // far down as they do.
        const difference = Math.round(cards.getBoundingClientRect().bottom - charts.getBoundingClientRect().bottom);
        if (difference > 3) found.push(`the charts end ${difference} px above the cards`);
      }
      return found;
    });
    for (const gap of gaps) summary.problems.push(`${key}: ${gap}`);
    await page.screenshot({ path: `${outputPrefix}-${key}.png`, fullPage: true });
    summary.pages[key] = await page.evaluate(() => document.querySelector("main").innerText);
    // What each visible day chart draws, read from the plot its element carries.
    summary.charts[key] = await page.evaluate(() =>
      [...document.querySelectorAll("[data-chart]")]
        .filter((element) => element.offsetParent !== null && element.solaxPlot)
        .map((element) => {
          const plot = element.solaxPlot;
          return {
            name: element.dataset.chart,
            labels: plot.series.slice(1).map((series) => series.label),
            data: plot.data,
            scales: Object.fromEntries(Object.entries(plot.scales).map(([name, scale]) => [name, [scale.min, scale.max]])),
          };
        }),
    );
    // Switching Generated on today shows its projection, and every chart on
    // Home moves out to where the projection ends, together.
    if (captureName === "today") {
      const generated = page.locator('.main-legend .item[data-metric="today_kwh"]');
      if ((await generated.count()) && (await generated.getAttribute("aria-pressed")) === "false") {
        await generated.click();
        await page.waitForTimeout(300);
        const switchedOn = await page.evaluate(() => {
          const plots = [...document.querySelectorAll(".screen:not([hidden]) [data-chart]")].map((element) => element.solaxPlot).filter(Boolean);
          const ranges = plots.map((plot) => [plot.scales.x.min, plot.scales.x.max]);
          const power = plots[0];
          const projected = power.series.findIndex((series) => series.label === "Projected");
          const shown = projected > 0 && power.series[projected].show;
          const projectedTimes = shown ? power.data[0].filter((_, index) => power.data[projected][index] !== null) : [];
          const end = projectedTimes.length ? projectedTimes[projectedTimes.length - 1] : null;
          const found = [];
          const onPace = document.querySelector("main").innerText.includes("On pace for");
          if (onPace && end === null) found.push("switching Generated on shows no projection");
          if (ranges.some(([min, max]) => Math.abs(min - ranges[0][0]) > 1 || Math.abs(max - ranges[0][1]) > 1)) found.push("with Generated on the charts show different time windows");
          if (end !== null && Math.abs(ranges[0][1] - end) > 1) found.push("with Generated on the charts do not run to the projection's end");
          return found;
        });
        for (const problem of switchedOn) summary.problems.push(`${key}: ${problem}`);
      }
    }
    await page.close();
  }
}
// On a touch screen no cursor moves before a tap, so tapping a Month bar has
// to find its day from the tap itself. The bar for the given day opens that
// day on Home.
{
  const page = await browser.newPage({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true, deviceScaleFactor: 3 });
  await page.goto(baseUrl + "/?token=change-me#/month/" + date.slice(0, 7));
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(1500);
  const point = await page.evaluate((day) => {
    const plot = document.querySelector(".screen:not([hidden]) .bars")?.solaxPlot;
    if (!plot) return null;
    const over = plot.over.getBoundingClientRect();
    const index = Number(day.slice(8)) - 1;
    return { x: over.left + plot.valToPos(plot.data[0][index], "x"), y: over.top + over.height * 0.7 };
  }, date);
  if (!point) {
    summary.problems.push("phone: the Month bars have no plot to tap");
  } else {
    await page.touchscreen.tap(point.x, point.y);
    await page.waitForTimeout(1500);
    const opened = await page.evaluate(() => ({
      hash: location.hash,
      day: document.querySelector(".screen:not([hidden]) .day-picker")?.value ?? null,
    }));
    if (opened.hash !== "#/" || opened.day !== date) {
      summary.problems.push(`phone: tapping the Month bar for ${date} opened ${opened.hash} on ${opened.day}`);
    }
  }
  await page.close();
}

await browser.close();
fs.writeFileSync(`${outputPrefix}-summary.json`, JSON.stringify(summary, null, 2));
console.log(`pages: ${Object.keys(summary.pages).length}, problems: ${summary.problems.length}`);
