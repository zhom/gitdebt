import { useSyncExternalStore } from "react";

export type RenderedTheme = "light" | "dark";

const listeners = new Set<() => void>();
let observing = false;

function snapshot(): RenderedTheme {
  if (typeof document === "undefined") return "light";
  const root = document.documentElement;
  return root.dataset.theme === "dark" ||
    root.dataset.darkreaderScheme === "dark"
    ? "dark"
    : "light";
}

function emit() {
  for (const listener of listeners) listener();
}

function startObserving() {
  if (observing || typeof document === "undefined") return;
  observing = true;
  const root = document.documentElement;
  const observer = new MutationObserver(emit);
  observer.observe(root, {
    attributes: true,
    attributeFilter: ["data-theme", "data-darkreader-scheme"],
  });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  startObserving();
  return () => listeners.delete(listener);
}

export function useRenderedTheme(): RenderedTheme {
  return useSyncExternalStore(subscribe, snapshot, () => "light");
}

