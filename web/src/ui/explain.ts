/**
 * One place for every scrap of explanatory copy in the UI, and the ⓘ control
 * that surfaces it. Splitting this copy across tooltips, slider hints, and the
 * NN labels is how half the panels ended up unexplained; this is the single
 * source of truth. `tooltips.ts` re-exports this map so existing `tipLabel`
 * call sites keep working unchanged.
 */

export const EXPLAIN: Record<string, string> = {
  // --- moved verbatim from tooltips.ts (keep keys identical) ---
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
    "An ant's success: food carried home (delivered) plus a small credit " +
    "(harvest_weight, ~0.02) for all the food it has ever picked up, plus a " +
    "credit (productivity_weight, ~0.1) for its recent productivity. Fitter " +
    "ants are chosen as parents more often.",
  harvested: "Lifetime food this ant has picked up (banked or not).",
  recentProductivity:
    "A decaying tally of food this ant has harvested or scavenged recently " +
    "(productivity_decay per tick). Rewards ants that keep producing, not just " +
    "ones that produced once long ago.",
  size: "Body size. Bigger ants cost more upkeep but hit harder.",
  "paid births": "Births paid for from the store (birth_cost each).",
  free: "Share of this colony's ants that were free extinction-floor spawns.",
  phFood: "Food-trail pheromone here: laid by laden ants, leads to food.",
  phAlarm: "Alarm pheromone here: spikes where ants were attacked.",
  phScent: "Territory scent here: the owning colony's claim on this cell.",
  phOwner: "Colony that owns the scent on this cell (none if unclaimed).",
  phHome: "Shared home/exploration trail strength on this cell.",
  nest: "Colony whose nest tile this is (none if open ground).",
  stone: "Stone coverage here (impassable).",
  food: "Standing food on this cell.",

  // --- new: playback / layers / world controls (Task 4) ---
  "ctl.pause": "Play or pause the simulation. Space also toggles it.",
  "ctl.step": "Advance exactly one tick, then stay paused. The way to watch the world — and any ant's brain — change one step at a time.",
  "ctl.speed": "Ticks per animation frame: 1×, 10×, or 100×. Higher burns through generations faster but you see less.",
  "layer.food": "Overlay the food-trail pheromone: laid by laden ants, points back to food.",
  "layer.alarm": "Overlay the alarm pheromone: spikes where ants were attacked.",
  "layer.scent": "Overlay territory scent: each colony's claim on the ground, tinted by owner.",
  "layer.home": "Overlay the shared home/exploration trail every ant lays and reads.",
  "layer.trail": "Overlay the fast-fading colony recent-path trail (own colony).",
  "ctl.labels": "Show colony name labels over each nest.",
  "ctl.pheroRes": "Pheromone texture resolution. 512² is sharper but heavier than 256².",
  "ctl.save": "Save the current world to the server's slot.",
  "ctl.load": "Reload the last saved world.",
  "ctl.reset": "Restart the world from the given seed. Same seed → same world.",

  // --- new: stats chart titles (Task 4) ---
  "stat.delivered": "Lifetime food carried home, summed across colonies. The core fitness signal.",
  "stat.population": "Ants alive per colony over time.",
  "stat.generation": "Mean lineage depth — how many births deep the living ants are, on average.",
  "stat.distinct": "How many distinct lineage depths are alive at once — a spread of generations.",
  "stat.refounds": "How many times a colony collapsed to zero and re-seeded from its hall of fame.",
  "stat.store": "Spendable colony food fund over time (births and refueling draw it down).",

  // --- new: readout rows without a key today (Task 4) ---
  "id": "Stable per-ant id, assigned at birth.",
  "age": "Ticks this ant has been alive.",
  "deaths": "Ants in this colony that have died, lifetime.",
  "name": "This ant's given name (cosmetic).",
  "colony": "Which colony this ant belongs to.",

  // --- new: section headings (Task 4) ---
  "sec.traits": "Fixed, heritable body/brain parameters set at birth — never change during life, only across generations.",
  "sec.inputs": "The 60 numbers the ant's network senses this tick.",
  "sec.outputs": "The 8 numbers the network produces each tick: a velocity command, attack, grab, and 4 recurrent memory values.",

  // --- tuning sliders (ids mirror CONFIG_FIELDS / tunables.ts) ---
  "tune.0": "Food-trail evaporation per tick. Nearer 1 = trails linger; the last decimal matters most.",
  "tune.1": "Alarm evaporation per tick. Nearer 1 = alarm lingers.",
  "tune.2": "Territory-scent evaporation per tick. Nearer 1 = claims persist.",
  "tune.3": "Food-trail diffusion: how much pheromone bleeds to neighboring cells each tick.",
  "tune.4": "Alarm diffusion per tick.",
  "tune.5": "Scent diffusion per tick.",
  "tune.6": "Energy tax per unit speed — the cost of moving fast.",
  "tune.7": "Energy tax per unit vision (×8: the trait ranges to 8). Seeing farther costs upkeep.",
  "tune.8": "Mutation rate: the chance each brain parameter is perturbed at birth.",
  "tune.9": "Mutation sigma: how large a perturbation is when it happens.",
  "tune.10": "Birth cost: food drawn from the store per paid birth. One foraging trip yields ~10.",
  "tune.11": "Harvest rate: food picked up per tick while standing on food.",
  "tune.12": "Refuel rate: energy restored per tick at the nest. High values let loiterers drain the store.",
  "tune.13": "Growth threshold: energy fraction an ant must hold before it spends any on growing.",
  "tune.14": "Ticks between food-relocation passes: how often depleted patches move elsewhere.",
  "tune.15": "Attack damage per successful bite.",
  "tune.16": "Harvest weight in fitness: 0 = deliver-only; nudging it up rewards picking food up at all.",
  "tune.17": "Homing weight: a fitness credit for carrying food toward home. Helps bootstrap foraging.",
  "tune.18": "Colony recent-path trail an ant lays each tick.",
  "tune.19": "Colony-trail evaporation. Fast decay = the trail means 'recent', not 'ever'.",
  "tune.20": "Colony-trail diffusion per tick.",
  "tune.21": "Productivity weight in fitness: rewards recent harvest/deliver/kills. 0 = cumulative only.",
  "tune.22": "Productivity decay: how fast 'recent' fades. 0.99 ≈ a 69-tick half-life.",
  "tune.23": "How many live food patches the world keeps on the map.",
};

let pinned: HTMLElement | null = null;

function removePop(): void {
  document.querySelector(".info-pop")?.remove();
  pinned = null;
}

function showPop(anchor: HTMLElement, text: string, pin: boolean): void {
  removePop();
  const pop = document.createElement("div");
  pop.className = "info-pop";
  pop.textContent = text;
  document.body.append(pop);
  const r = anchor.getBoundingClientRect();
  // Measure then flip so it never clips the viewport edge.
  let left = r.left;
  let top = r.bottom + 6;
  if (left + pop.offsetWidth > window.innerWidth - 6) {
    left = window.innerWidth - pop.offsetWidth - 6;
  }
  if (top + pop.offsetHeight > window.innerHeight - 6) {
    top = r.top - pop.offsetHeight - 6;
  }
  pop.style.left = `${Math.max(6, left)}px`;
  pop.style.top = `${Math.max(6, top)}px`;
  if (pin) pinned = pop;
}

export function explainText(key: string): string | undefined {
  return EXPLAIN[key];
}

export function infoDot(key: string): HTMLElement {
  const el = document.createElement("span");
  el.className = "info-dot";
  el.textContent = "ⓘ";
  el.setAttribute("tabindex", "0");
  const text = EXPLAIN[key];
  if (!text) {
    // Fail soft: an unknown key is a dead, silent dot, never a thrown render.
    if (import.meta.env?.DEV) console.warn(`infoDot: no copy for "${key}"`);
    el.classList.add("info-dot-empty");
    return el;
  }
  el.setAttribute("data-info", text);

  el.addEventListener("mouseenter", () => {
    if (!pinned) showPop(el, text, false);
  });
  el.addEventListener("mouseleave", () => {
    if (!pinned) removePop();
  });
  const toggle = (e: Event) => {
    e.stopPropagation();
    if (pinned) removePop();
    else showPop(el, text, true);
  };
  el.addEventListener("click", toggle);
  el.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle(e);
    }
  });
  return el;
}

// A pinned popover closes on the next outside click or Escape. Guarded so this
// module can still be imported (e.g. by tooltips.ts) under plain-node tests
// that never touch the DOM.
if (typeof document !== "undefined") {
  document.addEventListener("click", () => {
    if (pinned) removePop();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && pinned) removePop();
  });
}
