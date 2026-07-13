/**
 * One-line, plain-English definitions for every number the panels show, in one
 * place. `tipLabel` builds a label span that reveals its definition on hover
 * (styled via the `[data-tip]` CSS in index.html, not the native title).
 */

export const TOOLTIPS: Record<string, string> = {
  pop: "Ants alive right now.",
  store:
    "Spendable food fund. Births and refueling draw from it. A colony at 0 " +
    "survives on ant energy reserves plus the extinction floor.",
  delivered:
    "Lifetime food carried home. An odometer — never spent down. This is the " +
    "colony's fitness signal.",
  energy: "The ant's personal fuel. Spent moving; refilled only at its own nest.",
  generation: "Lineage depth — how many births deep this line is.",
  carrying: "Food the ant is holding, not yet banked at a nest.",
  fitness:
    "This ant's success: food carried home (delivered) plus a small 2% credit " +
    "for food it is still holding. Fitter ants are chosen as parents more often.",
  harvested: "Lifetime food this ant has picked up (banked or not).",
  size: "Body size. Bigger ants cost more upkeep but hit harder.",
  "paid births": "Births paid for from the store (birth_cost each).",
  free: "Share of this colony's ants that were free extinction-floor spawns.",
  phFood: "Food-trail pheromone here: laid by laden ants, leads to food.",
  phAlarm: "Alarm pheromone here: spikes where ants were attacked.",
  phScent: "Territory scent here: the owning colony's claim on this cell.",
  phOwner: "Colony that owns the scent on this cell (none if unclaimed).",
  nest: "Colony whose nest tile this is (none if open ground).",
  stone: "Stone coverage here (impassable).",
  food: "Standing food on this cell.",
};

/** A label span that shows its tooltip on hover. `key` selects the copy. */
export function tipLabel(text: string, key: string): HTMLSpanElement {
  const el = document.createElement("span");
  el.textContent = text;
  const tip = TOOLTIPS[key];
  if (tip) {
    el.className = "tip";
    el.setAttribute("data-tip", tip);
  }
  return el;
}
