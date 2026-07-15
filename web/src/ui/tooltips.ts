import { EXPLAIN } from "./explain.js";

/**
 * One-line, plain-English definitions for every number the panels show, in one
 * place. `tipLabel` builds a label span that reveals its definition on hover
 * (styled via the `[data-tip]` CSS in index.html, not the native title).
 *
 * @deprecated Use `explain.ts` / `infoDot` directly. Kept so existing
 *  `tipLabel(text, key)` call sites resolve against the one registry.
 */
export const TOOLTIPS: Record<string, string> = EXPLAIN;

/** A label span that shows its tooltip on hover. `key` selects the copy. */
export function tipLabel(text: string, key: string): HTMLSpanElement {
  return tipText(text, TOOLTIPS[key]);
}

/** Like `tipLabel` but with the tooltip copy passed in directly. */
export function tipText(text: string, tip: string | undefined): HTMLSpanElement {
  const el = document.createElement("span");
  el.textContent = text;
  if (tip) {
    el.className = "tip";
    el.setAttribute("data-tip", tip);
  }
  return el;
}
