import assert from "node:assert/strict";
import { test } from "node:test";

import {
  installPreferredSource,
  loadPreferredSourceScript,
  PREFERRED_SOURCE_COPY,
  PREFERRED_SOURCE_SCRIPT_ID,
  PREFERRED_SOURCE_SCRIPT_SRC,
} from "../src/lib/preferred-source.ts";
import { installEmbedMode, leavingHref } from "../src/lib/embed-mode.ts";

/** A document with just enough surface for the module: head, ids, clicks, root attributes. */
function fakeDocument({ theme = null, lang = "en", dark = false } = {}) {
  const attributes = new Map(theme ? [["data-theme", theme]] : []);
  const classes = new Set(dark ? ["dark"] : []);
  const appended = [];
  const listeners = [];
  const doc = {
    appended,
    documentElement: {
      lang,
      getAttribute: (name) => attributes.get(name) ?? null,
      setAttribute: (name, value) => attributes.set(name, value),
      classList: {
        contains: (token) => classes.has(token),
        add: (token) => classes.add(token),
      },
    },
    head: { appendChild: (node) => appended.push(node) },
    getElementById: (id) => appended.find((node) => node.id === id) ?? null,
    createElement: () => {
      const attributes = {};
      return {
        id: "",
        src: "",
        async: false,
        attributes,
        setAttribute: (name, value) => {
          attributes[name] = value;
        },
      };
    },
    addEventListener: (type, listener) => listeners.push({ type, listener }),
    click(target) {
      for (const { type, listener } of listeners) {
        if (type === "click") listener({ target });
      }
    },
  };
  return doc;
}

const button = { closest: (selector) => (selector === "[data-prefer-on-google]" ? button : null) };
const elsewhere = { closest: () => null };

function fakeApi() {
  const calls = { init: [], add: 0 };
  return {
    calls,
    init: (options) => calls.init.push(options),
    addPreferredSource: () => {
      calls.add += 1;
    },
  };
}

/** What the library does when it boots: drain the queue, then run later pushes at once. */
function flushQueue(host, api) {
  const queued = host.PREFERRED_SOURCE ?? [];
  host.PREFERRED_SOURCE = [];
  for (const callback of queued) callback(api);
}

test("loads the publisher script once, async, with manual control", () => {
  const doc = fakeDocument();
  loadPreferredSourceScript(doc);
  loadPreferredSourceScript(doc);
  installPreferredSource({}, doc);
  assert.equal(doc.appended.length, 1);
  const [script] = doc.appended;
  assert.equal(script.id, PREFERRED_SOURCE_SCRIPT_ID);
  assert.equal(script.src, PREFERRED_SOURCE_SCRIPT_SRC);
  assert.equal(script.async, true);
  assert.equal(script.attributes["preferred-sources-control"], "manual");
});

test("initialises with the page's theme and language", () => {
  const host = {};
  const api = fakeApi();
  installPreferredSource(host, fakeDocument({ theme: "dark", lang: "de" }));
  flushQueue(host, api);
  assert.deepEqual(api.calls.init, [{ theme: "dark", lang: "de" }]);

  const lightHost = {};
  const lightApi = fakeApi();
  installPreferredSource(lightHost, fakeDocument());
  flushQueue(lightHost, lightApi);
  assert.deepEqual(lightApi.calls.init, [{ theme: "light", lang: "en" }]);
});

test("a click on the button starts Google's flow; other clicks do not", () => {
  const host = {};
  const api = fakeApi();
  const doc = fakeDocument();
  installPreferredSource(host, doc);
  flushQueue(host, api);
  doc.click(elsewhere);
  doc.click(button);
  assert.equal(api.calls.add, 1);
});

test("a click before the library loads is replayed once it is ready", () => {
  const host = {};
  const api = fakeApi();
  const doc = fakeDocument();
  installPreferredSource(host, doc);
  doc.click(button);
  assert.equal(api.calls.add, 0);
  flushQueue(host, api);
  assert.equal(api.calls.add, 1);
});

test("init runs again when the theme changes, and only then", () => {
  const host = {};
  const api = fakeApi();
  const doc = fakeDocument();
  let onChange = () => {};
  installPreferredSource(host, doc, (callback) => {
    onChange = callback;
    return { observe: () => {} };
  });
  flushQueue(host, api);
  onChange();
  flushQueue(host, api);
  assert.equal(api.calls.init.length, 1);

  doc.documentElement.setAttribute("data-theme", "dark");
  onChange();
  flushQueue(host, api);
  assert.deepEqual(api.calls.init.at(-1), { theme: "dark", lang: "en" });
});

test("copy names Google and the brand, and makes no ranking claim", () => {
  for (const copy of Object.values(PREFERRED_SOURCE_COPY)) {
    assert.ok(copy.button.length <= 30);
    assert.match(copy.button, /Google/);
    assert.match(copy.description, /gitdebt/);
    assert.doesNotMatch(
      `${copy.heading} ${copy.description}`,
      /\b(rank|ranking|boost|SEO|guarantee)/i,
    );
  }
});

function click(target, overrides = {}) {
  return {
    button: 0,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    defaultPrevented: false,
    target,
    prevented: false,
    preventDefault() {
      this.prevented = true;
    },
    ...overrides,
  };
}

function anchor(href, absolute) {
  const node = { getAttribute: () => href, href: absolute };
  return { closest: () => node };
}

test("embed mode sends links to a new tab only when the page is framed", () => {
  const opened = [];
  const framedWindow = {
    self: {},
    top: {},
    open: (url, target, features) => opened.push([url, target, features]),
  };
  const listeners = [];
  const classes = new Set();
  const doc = {
    documentElement: { classList: { add: (token) => classes.add(token) } },
    addEventListener: (_type, listener, capture) => listeners.push({ listener, capture }),
  };
  installEmbedMode(framedWindow, doc);
  assert.ok(classes.has("framed"));
  assert.equal(listeners[0].capture, true);

  const event = click(anchor("/facebook/react", "https://gitdebt.com/facebook/react"));
  listeners[0].listener(event);
  assert.equal(event.prevented, true);
  assert.deepEqual(opened, [
    ["https://gitdebt.com/facebook/react", "_blank", "noopener"],
  ]);

  assert.equal(leavingHref(click(anchor("#main", "https://gitdebt.com/#main"))), null);
  assert.equal(
    leavingHref(click(anchor("/a", "https://gitdebt.com/a"), { metaKey: true })),
    null,
  );

  const top = {};
  const unframedListeners = [];
  installEmbedMode(
    { self: top, top, open: () => {} },
    {
      documentElement: { classList: { add: () => assert.fail("not framed") } },
      addEventListener: () => unframedListeners.push(1),
    },
  );
  assert.equal(unframedListeners.length, 0);
});
