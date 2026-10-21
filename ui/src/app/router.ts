import { monthOf } from "../periods";

export type Tab = "home" | "month" | "year";

export const TABS: Tab[] = ["home", "month", "year"];

export interface Route {
  tab: Tab;
  /** YYYY-MM for Month, YYYY for Year. */
  param: string | null;
}

const TAB_ICONS: Record<Tab, string> = {
  home: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/></svg>',
  month: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/></svg>',
  year: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 20V12M9 20V8M14 20v-5M19 20V5"/></svg>',
};

const TAB_TITLES: Record<Tab, string> = { home: "Home", month: "Month", year: "Year" };

/** The tab an address opens. Anything else, the old Details address
 * included, is Home, which holds everything Details had. */
export function parseRoute(hash: string): Route {
  const [, tab = "", param = null] = hash.replace(/^#/, "").split("/");
  if (tab === "month") return { tab: "month", param: /^\d{4}-\d{2}$/.test(param ?? "") ? param : null };
  if (tab === "year") return { tab: "year", param: /^\d{4}$/.test(param ?? "") ? param : null };
  return { tab: "home", param: null };
}

export function addressOf(route: Route, today: string): string {
  switch (route.tab) {
    case "home": return "#/";
    case "month": return "#/month/" + (route.param ?? monthOf(today));
    case "year": return "#/year/" + (route.param ?? today.slice(0, 4));
  }
}

/** Builds the tab links once; keeps their targets and the current tab marked. */
export function paintTabs(container: HTMLElement, current: Route, today: string, remembered: Partial<Record<Tab, string>>): void {
  if (!container.children.length) {
    for (const tab of TABS) {
      const link = document.createElement("a");
      link.dataset.tab = tab;
      link.innerHTML = TAB_ICONS[tab] + `<span>${TAB_TITLES[tab]}</span>`;
      container.appendChild(link);
    }
  }
  for (const link of container.querySelectorAll<HTMLAnchorElement>("a")) {
    const tab = link.dataset.tab as Tab;
    link.href = remembered[tab] ?? addressOf({ tab, param: null }, today);
    if (tab === current.tab) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
}
