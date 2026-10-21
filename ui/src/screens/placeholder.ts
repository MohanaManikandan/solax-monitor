import type { Screen } from "./screen";

/** A screen that is not built yet, so routing works end to end meanwhile. */
export function placeholderScreen(name: string): Screen {
  const element = document.createElement("section");
  element.className = "screen";
  element.dataset.screen = name;
  element.hidden = true;
  element.innerHTML = '<p class="muted">This screen is on its way.</p>';
  return {
    element,
    show() {
      element.hidden = false;
    },
    hide() {
      element.hidden = true;
    },
  };
}
