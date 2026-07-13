/**
 * The right rail is two tabs: Colonies (overview) and Explorer (whatever is
 * selected). The tab strip reflects `store.state.activeTab`, which selecting an
 * entity flips to "explorer", so a click in the world brings its inspector up
 * without the operator hunting for the tab.
 */

import type { Store } from "../state.js";

export function mountRail(
  root: HTMLElement,
  store: Store,
): { coloniesPane: HTMLElement; explorerPane: HTMLElement } {
  const strip = document.createElement("div");
  strip.className = "tabstrip";

  const coloniesPane = document.createElement("div");
  const explorerPane = document.createElement("div");
  coloniesPane.className = "tabpane";
  explorerPane.className = "tabpane";

  const tabs: { key: "colonies" | "explorer"; label: string; pane: HTMLElement }[] = [
    { key: "colonies", label: "Colonies", pane: coloniesPane },
    { key: "explorer", label: "Explorer", pane: explorerPane },
  ];
  const btns = tabs.map((t) => {
    const b = document.createElement("button");
    b.className = "tab";
    b.textContent = t.label;
    b.addEventListener("click", () => store.setTab(t.key));
    strip.append(b);
    return b;
  });

  root.append(strip, coloniesPane, explorerPane);

  const sync = () => {
    const active = store.state.activeTab;
    tabs.forEach((t, i) => {
      const on = t.key === active;
      btns[i].classList.toggle("on", on);
      t.pane.style.display = on ? "" : "none";
    });
  };
  store.subscribe(sync);
  sync();

  return { coloniesPane, explorerPane };
}
