/**
 * A dependency-free right-click menu. Items are either an immediate action or
 * an inline editor (a small input + Apply) for the amount/name a command needs.
 * Closes on outside-click or Escape. Only one menu is ever open.
 */

export interface MenuItem {
  label: string;
  /** Fired immediately on click. Omit when `editor` is set. */
  onClick?: () => void;
  /** Reveals an inline input; `apply` receives the raw string. */
  editor?: { placeholder: string; initial?: string; apply: (value: string) => void };
}

let open: HTMLElement | null = null;
let onDocClick: ((e: MouseEvent) => void) | null = null;
let onKey: ((e: KeyboardEvent) => void) | null = null;

export function closeContextMenu(): void {
  if (open) {
    open.remove();
    open = null;
  }
  if (onDocClick) {
    document.removeEventListener("pointerdown", onDocClick, true);
    onDocClick = null;
  }
  if (onKey) {
    document.removeEventListener("keydown", onKey, true);
    onKey = null;
  }
}

export function openContextMenu(x: number, y: number, items: MenuItem[]): void {
  closeContextMenu();

  const menu = document.createElement("div");
  menu.className = "ctx-menu";
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;

  for (const item of items) {
    const row = document.createElement("div");
    row.className = "ctx-item";
    row.textContent = item.label;
    if (item.onClick) {
      row.addEventListener("click", () => {
        item.onClick!();
        closeContextMenu();
      });
    } else if (item.editor) {
      // Clicking swaps the label for an inline input, so a value can be typed
      // without a browser prompt() blocking the render loop.
      row.addEventListener("click", (e) => {
        e.stopPropagation();
        if (row.querySelector("input")) return;
        row.textContent = "";
        const input = document.createElement("input");
        input.className = "ctx-input";
        input.placeholder = item.editor!.placeholder;
        if (item.editor!.initial !== undefined) input.value = item.editor!.initial;
        const go = () => {
          item.editor!.apply(input.value);
          closeContextMenu();
        };
        const apply = document.createElement("button");
        apply.className = "ctx-apply";
        apply.textContent = "OK";
        apply.addEventListener("click", (ev) => {
          ev.stopPropagation();
          go();
        });
        input.addEventListener("keydown", (ev) => {
          if (ev.key === "Enter") go();
          ev.stopPropagation();
        });
        row.append(input, apply);
        input.focus();
      });
    }
    menu.append(row);
  }

  document.body.append(menu);
  open = menu;

  // Keep the menu on-screen when opened near the right/bottom edge.
  const r = menu.getBoundingClientRect();
  if (r.right > window.innerWidth) menu.style.left = `${x - r.width}px`;
  if (r.bottom > window.innerHeight) menu.style.top = `${y - r.height}px`;

  onDocClick = (e: MouseEvent) => {
    if (open && !open.contains(e.target as Node)) closeContextMenu();
  };
  onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") closeContextMenu();
  };
  // Capture so an outside pointerdown closes before it hits the canvas.
  document.addEventListener("pointerdown", onDocClick, true);
  document.addEventListener("keydown", onKey, true);
}
