import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const contentSrc = readFileSync(join(root, "content.js"), "utf8");

function extractFn(src, name) {
  const start = src.indexOf("function " + name + "(");
  assert.notEqual(start, -1, `could not find function ${name}`);
  const open = src.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    if (src[i] !== "}") continue;
    depth--;
    if (depth === 0) {
      const body = src.slice(start, i + 1);
      return new Function(`${body}\nreturn ${name};`)();
    }
  }
  throw new Error(`unbalanced braces extracting ${name}`);
}

function fakeElement() {
  const attributes = new Map();
  const listeners = new Map();
  return {
    attributes,
    dataset: {},
    hidden: false,
    style: {
      removeProperty(name) {
        delete this[name];
      }
    },
    setAttribute(name, value) {
      attributes.set(name, String(value));
    },
    getAttribute(name) {
      return attributes.get(name) ?? null;
    },
    removeAttribute(name) {
      attributes.delete(name);
    },
    addEventListener(name, listener) {
      const list = listeners.get(name) || [];
      list.push(listener);
      listeners.set(name, list);
    },
    dispatch(name, detail = {}) {
      for (const listener of listeners.get(name) || []) {
        listener({ target: this, ...detail });
      }
    }
  };
}

function fakeRuntime({ reduced = false } = {}) {
  let nextId = 1;
  const frames = new Map();
  const timers = new Map();
  const computed = {
    opacity: "0",
    transform: "translateY(-4px) scale(0.98)"
  };

  return {
    runtime: {
      reduceMotion: () => reduced,
      getComputedStyle: () => computed,
      requestAnimationFrame(callback) {
        const id = nextId++;
        frames.set(id, callback);
        return id;
      },
      cancelAnimationFrame(id) {
        frames.delete(id);
      },
      setTimeout(callback, delay) {
        const id = nextId++;
        timers.set(id, { callback, delay });
        return id;
      },
      clearTimeout(id) {
        timers.delete(id);
      }
    },
    computed,
    flushFrames() {
      const pending = [...frames.values()];
      frames.clear();
      for (const callback of pending) callback();
    },
    flushTimers() {
      const pending = [...timers.values()];
      timers.clear();
      for (const { callback } of pending) callback();
    },
    timerCount() {
      return timers.size;
    },
    timerDelays() {
      return [...timers.values()].map(({ delay }) => delay);
    }
  };
}

const createDisclosureController = extractFn(
  contentSrc,
  "createDisclosureController"
);

function setup(options) {
  const toggle = fakeElement();
  const panel = fakeElement();
  const clock = fakeRuntime(options);
  let loads = 0;

  toggle.setAttribute("aria-expanded", "false");
  panel.hidden = true;
  panel.dataset.state = "closed";
  panel.setAttribute("aria-hidden", "true");
  panel.setAttribute("inert", "");

  const controller = createDisclosureController(
    toggle,
    panel,
    () => {
      loads += 1;
    },
    clock.runtime
  );

  return {
    toggle,
    panel,
    clock,
    controller,
    loads: () => loads
  };
}

test("disclosure updates ARIA immediately and delays hidden until exit ends", () => {
  const { toggle, panel, clock, controller, loads } = setup();

  controller.toggle();
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(panel.hidden, false);
  assert.equal(panel.dataset.state, "opening");
  assert.equal(panel.getAttribute("aria-hidden"), null);
  assert.equal(panel.getAttribute("inert"), null);
  assert.equal(loads(), 1);

  clock.flushFrames();
  panel.dispatch("transitionend", { propertyName: "opacity" });
  assert.equal(panel.dataset.state, "open");

  controller.toggle();
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(panel.hidden, false);
  assert.equal(panel.dataset.state, "closing");
  assert.equal(panel.getAttribute("aria-hidden"), "true");
  assert.equal(panel.getAttribute("inert"), "");

  clock.flushFrames();
  assert.equal(panel.hidden, false);
  assert.deepEqual(clock.timerDelays(), [200]);
  clock.flushTimers();
  assert.equal(panel.hidden, true);
  assert.equal(panel.dataset.state, "closed");
});

test("rapid reversal cancels stale frames and fallbacks", () => {
  const { toggle, panel, clock, controller, loads } = setup();

  controller.toggle();
  clock.flushFrames();
  assert.equal(clock.timerCount(), 1);

  clock.computed.opacity = "0.55";
  clock.computed.transform = "matrix(0.99, 0, 0, 0.99, 0, -2)";
  controller.toggle();
  clock.flushFrames();
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(clock.timerCount(), 1);

  clock.computed.opacity = "0.25";
  clock.computed.transform = "matrix(0.985, 0, 0, 0.985, 0, -3)";
  controller.toggle();
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(panel.hidden, false);
  assert.equal(panel.style.opacity, "0.25");
  assert.equal(loads(), 1);

  clock.flushFrames();
  assert.equal(clock.timerCount(), 1);
  clock.flushTimers();
  assert.equal(panel.dataset.state, "open");
  assert.equal(panel.hidden, false);
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
});

test("reduced motion keeps the disclosure opacity-only", () => {
  const { panel, clock, controller } = setup({ reduced: true });

  controller.toggle();
  clock.flushFrames();

  assert.equal(panel.style.transform, "none");
  assert.match(panel.style.transition, /^opacity 140ms /);
  assert.doesNotMatch(panel.style.transition, /transform/);
});

test("caret rotation and disclosure states are stylesheet-driven", () => {
  assert.ok(contentSrc.includes('text: "▸"'));
  assert.equal(contentSrc.includes('textContent = open ? "▸" : "▾"'), false);
  assert.match(
    contentSrc,
    /\.gd-toggle\[aria-expanded="true"\] \.gd-toggle-caret \{ transform: rotate\(90deg\); \}/
  );
  assert.match(contentSrc, /@media \(prefers-reduced-motion: reduce\)/);
});
