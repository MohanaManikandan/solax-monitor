import { barsHeight } from "../charts/sizing";

const MARKUP = `
  <section class="card hero summary-card loading">
    <div class="period-head">
      <div class="day-nav">
        <button type="button" class="period-prev" title="Earlier">&lsaquo;</button>
        <h2 class="period-title"></h2>
        <button type="button" class="period-next" title="Later">&rsaquo;</button>
      </div>
    </div>
    <div class="period-body">
      <p class="period-caption">Generated</p>
      <div class="hero-number"><span class="period-sum">–</span><span class="hero-unit period-unit"></span></div>
      <p class="period-average"></p>
      <p class="hero-line period-comparison" hidden></p>
    </div>
    <div class="period-empty" hidden>
      <p class="period-sentence"></p>
      <div class="day-nav"><button type="button" class="period-nearest text"></button></div>
    </div>
  </section>
  <section class="card bars-card">
    <div class="bars"></div>
  </section>
  <section class="tiles"></section>
`;

/** One figure as a card of its own: its value in large type, what it is
 * above, and a detail such as the day it happened on below. */
export interface Tile {
  label: string;
  value: string;
  detail: string | null;
}

/** A number with its grouping and decimal marks, as the locale writes it. */
const NUMBER = /(\d(?:[\d,.\u00a0\u202f]*\d)?)/;

/** A value cut into its numbers and the words between them, so the words
 * can be set small beside the numbers. */
export function valueParts(value: string): Array<{ text: string; unit: boolean }> {
  return value
    .split(NUMBER)
    .filter((text) => text !== "")
    .map((text) => ({ text, unit: !NUMBER.test(text) }));
}

export interface PeriodFrame {
  readonly title: HTMLElement;
  readonly previous: HTMLButtonElement;
  readonly next: HTMLButtonElement;
  readonly average: HTMLElement;
  readonly barsElement: HTMLElement;
  /** The period's figures, one tile each. */
  setTiles(tiles: Tile[]): void;
  /** The total, set in the hero card's large type with its unit beside it. */
  setTotal(text: string): void;
  setComparison(text: string | null): void;
  /** The period's figures and bars. */
  showContent(): void;
  /** Instead of the period, a sentence and maybe a button to another one;
   * the summary then takes the whole screen. */
  showMessage(sentence: string, nearest: { target: string; label: string } | null): void;
  /** How tall the bars are, measured from where they sit: on a computer
   * they reach down to the footer. */
  barsHeight(): number;
}

/**
 * A Month or Year screen: a summary hero card with the period between its
 * arrows, the total in large type, its average and a plain comparison; the
 * bars in a card of their own; and the figures as tiles. On a computer the
 * tiles sit under the summary with the bars beside both, as Home's cards
 * sit under its hero; on a phone the summary, the bars and the tiles stack.
 */
export function createPeriodFrame(screen: HTMLElement, goTo: (target: string) => void): PeriodFrame {
  screen.innerHTML = MARKUP;
  const find = <Found extends HTMLElement>(selector: string) => screen.querySelector(selector) as Found;
  const summaryCard = find(".summary-card");
  const barsCard = find(".bars-card");
  const body = find(".period-body");
  const sum = find(".period-sum");
  const unit = find(".period-unit");
  const comparison = find(".period-comparison");
  const tiles = find(".tiles");
  const empty = find(".period-empty");
  const sentence = find(".period-sentence");
  const nearest = find<HTMLButtonElement>(".period-nearest");
  const barsElement = find(".bars");
  nearest.addEventListener("click", () => {
    if (nearest.dataset.target) goTo(nearest.dataset.target);
  });

  return {
    title: find(".period-title"),
    previous: find<HTMLButtonElement>(".period-prev"),
    next: find<HTMLButtonElement>(".period-next"),
    average: find(".period-average"),
    barsElement,
    setTiles(list) {
      tiles.replaceChildren(
        ...list.map((tile) => {
          const card = document.createElement("section");
          card.className = "card tile";
          card.innerHTML = '<p class="tile-label"></p><p class="tile-value"></p><p class="tile-detail"></p>';
          card.querySelector(".tile-label")!.textContent = tile.label;
          card.querySelector(".tile-value")!.append(
            ...valueParts(tile.value).map((part) => {
              if (!part.unit) return part.text;
              const unit = document.createElement("span");
              unit.className = "unit";
              unit.textContent = part.text;
              return unit;
            }),
          );
          const detail = card.querySelector(".tile-detail") as HTMLElement;
          detail.textContent = tile.detail ?? "";
          detail.hidden = tile.detail === null;
          return card;
        }),
      );
    },
    setTotal(text) {
      const split = text.lastIndexOf(" ");
      sum.textContent = split > 0 ? text.slice(0, split) : text;
      unit.textContent = split > 0 ? text.slice(split + 1) : "";
    },
    setComparison(text) {
      comparison.textContent = text ?? "";
      comparison.hidden = text === null;
    },
    showContent() {
      summaryCard.classList.remove("loading");
      screen.classList.remove("empty");
      body.hidden = false;
      empty.hidden = true;
      barsCard.hidden = false;
      tiles.hidden = false;
    },
    showMessage(text, target) {
      summaryCard.classList.remove("loading");
      screen.classList.add("empty");
      body.hidden = true;
      empty.hidden = false;
      barsCard.hidden = true;
      tiles.hidden = true;
      sentence.textContent = text;
      nearest.hidden = target === null;
      nearest.dataset.target = target?.target ?? "";
      nearest.textContent = target?.label ?? "";
    },
    barsHeight() {
      const page = document.querySelector(".page");
      const pageStyle = page ? getComputedStyle(page) : null;
      const footer = document.querySelector(".footer");
      const legend = barsElement.querySelector(".u-legend");
      const cardStyle = getComputedStyle(barsCard);
      const inCard =
        (legend instanceof HTMLElement ? legend.offsetHeight : 32) +
        (parseFloat(cardStyle.paddingBottom) || 0) +
        (parseFloat(cardStyle.borderBottomWidth) || 0);
      const below =
        inCard +
        (parseFloat(pageStyle?.rowGap ?? "") || 16) +
        (footer instanceof HTMLElement ? footer.offsetHeight : 0) +
        (parseFloat(pageStyle?.paddingBottom ?? "") || 0) +
        1;
      // The tiles end the column beside the bars on a computer; the
      // summary does when there are none.
      const column = tiles.childElementCount ? tiles : summaryCard;
      return barsHeight({
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
        barsTop: barsElement.getBoundingClientRect().top + window.scrollY,
        below,
        inCard,
        columnBottom: column.getBoundingClientRect().bottom + window.scrollY,
      });
    },
  };
}

/** The sentence shown when the day rows behind a period could not be loaded. */
export function historyErrorSentence(error: string): string {
  return "The day history could not be loaded (" + error + "). It is tried again when the page reconnects.";
}
