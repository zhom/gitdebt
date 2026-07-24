(() => {
  "use strict";

  const ext = globalThis.chrome || globalThis.browser;

  if (window.__gitdebtInjected) return;
  window.__gitdebtInjected = true;

  const DEFAULT_API_BASE = "https://api.gitdebt.com";
  const SITE_BASE = "https://gitdebt.com";
  const PANEL_ID = "gitdebt-panel-host";

  const RESERVED_FIRST_SEGMENTS = new Set([
    "features", "topics", "collections", "sponsors", "marketplace",
    "settings", "notifications", "explore", "about", "pricing", "login",
    "join", "new", "organizations", "orgs", "users", "search", "trending",
    "dashboard", "watching", "stars", "issues", "pulls", "codespaces",
    "apps", "github-copilot", "copilot", "account", "logout", "signup",
    "contact", "site", "security", "enterprise", "team", "customer-stories",
    "readme", "nonprofit", "education", "premium-support", "git-guides",
    "mobile", "discussions", "sponsors-waitlist", "assets", "favicons",
    "404", "home", "tos", "privacy"
  ]);

  const STAT_CHARTS = [
    { name: "contributors", label: "Contributors" },
    { name: "lines", label: "Language lines" },
    { name: "top-files", label: "File churn" },
    { name: "bug-magnets", label: "Bug magnets" },
    { name: "heatmap", label: "Commit heatmap" },
    { name: "todo-trend", label: "TODO/FIXME trend" },
    { name: "bus-factor", label: "Bus factor" },
    { name: "commit-trend", label: "Commit trend" }
  ];

  const POLL_BASE_MS = 4000;
  const POLL_MAX_MS = 8000;
  // The panel reaches a truthful terminal state ("still gathering — open the
  // full report") on its own, so a long tail of polls buys nothing.
  const POLL_MAX_ATTEMPTS = 12;
  const MOUNT_WAIT_MS = 8000;
  const ANALYZE_TIMEOUT_MS = 15000;

  const warnedOnce = new Set();
  function warnOnce(key, ...args) {
    if (warnedOnce.has(key)) return;
    warnedOnce.add(key);
    try { console.warn("[gitdebt]", ...args); } catch (_) { /* ignore */ }
  }

  function el(tag, attrs, children) {
    const node = document.createElement(tag);
    if (attrs) {
      for (const k in attrs) {
        if (k === "text") node.textContent = attrs[k];
        else if (attrs[k] != null) node.setAttribute(k, attrs[k]);
      }
    }
    if (children) {
      for (const c of [].concat(children)) {
        if (c == null) continue;
        node.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
      }
    }
    return node;
  }

  function getSettings() {
    return new Promise((resolve) => {
      const fallback = { enabled: true };
      try {
        ext.storage.sync.get({ enabled: true }, (items) => {
          if (ext.runtime && ext.runtime.lastError) { resolve(fallback); return; }
          resolve({
            enabled: !items || items.enabled !== false
          });
        });
      } catch (_) { resolve(fallback); }
    });
  }

  function resolveRepo() {
    try {
      const parts = location.pathname.split("/").filter(Boolean);
      if (parts.length < 2) return null;
      const owner = decodeURIComponent(parts[0]);
      const repo = decodeURIComponent(parts[1]);
      if (RESERVED_FIRST_SEGMENTS.has(owner.toLowerCase())) return null;
      const nameRe = /^[A-Za-z0-9_.-]+$/;
      if (!nameRe.test(owner) || !nameRe.test(repo)) return null;
      if (repo === "." || repo === "..") return null;
      return { owner, repo };
    } catch (_) {
      return null;
    }
  }

  function isPublicRepoPage() {
    const meta = document.querySelector(
      'meta[name="octolytics-dimension-repository_public"]'
    );
    if (meta) return meta.content.toLowerCase() === "true";

    const header = document.querySelector("#repository-container-header");
    if (!header) return false;
    for (const label of header.querySelectorAll(".Label")) {
      const visibility = label.textContent.trim().toLowerCase();
      if (visibility === "public") return true;
      if (visibility === "private") return false;
    }
    return false;
  }

  try {
    ext.runtime.onMessage.addListener((message, _sender, sendResponse) => {
      if (!message || message.type !== "gitdebt:get-repo-context") return;
      sendResponse({
        isPublic: Boolean(resolveRepo() && isPublicRepoPage())
      });
    });
  } catch (_) {}

  function parseCount(raw) {
    if (raw == null) return null;
    const s = String(raw).trim().toLowerCase().replace(/,/g, "");
    if (!s) return null;
    const m = s.match(/^([0-9]*\.?[0-9]+)\s*([km])?$/);
    if (!m) {
      const n = Number(s);
      return Number.isFinite(n) ? Math.round(n) : null;
    }
    let n = parseFloat(m[1]);
    if (!Number.isFinite(n)) return null;
    if (m[2] === "k") n *= 1000;
    else if (m[2] === "m") n *= 1000000;
    return Math.round(n);
  }

  function countFromNode(node) {
    if (!node) return null;
    const fromTitle = node.getAttribute && parseCount(node.getAttribute("title"));
    if (fromTitle != null) return fromTitle;
    const fromAria = node.getAttribute && parseCount(node.getAttribute("aria-label"));
    if (fromAria != null) return fromAria;
    return parseCount(node.textContent);
  }

  function readStarCount(owner, repo) {
    try {
      const primary = countFromNode(document.querySelector("#repo-stars-counter-star"));
      if (primary != null) return primary;

      const selectors = [];
      if (owner && repo) {
        selectors.push('a[href="/' + owner + "/" + repo + '/stargazers"]');
      }
      selectors.push('a[href$="/stargazers"]');
      for (const sel of selectors) {
        let link;
        try { link = document.querySelector(sel); } catch (_) { continue; }
        if (!link) continue;
        const inner = link.querySelector(".Counter, [data-view-component]") || link;
        const n = countFromNode(inner);
        if (n != null) return n;
      }
    } catch (_) { /* fall through */ }
    return null;
  }

  function waitForStarCount(owner, repo, token, maxMs) {
    return new Promise((resolve) => {
      const immediate = readStarCount(owner, repo);
      if (immediate != null) { resolve(immediate); return; }
      let done = false;
      let obs = null;
      const finish = (v) => {
        if (done) return;
        done = true;
        if (obs) obs.disconnect();
        clearTimeout(timer);
        resolve(v);
      };
      const timer = setTimeout(() => finish(readStarCount(owner, repo)), maxMs);
      obs = new MutationObserver(() => {
        if (token !== currentRunToken) { finish(null); return; }
        const v = readStarCount(owner, repo);
        if (v != null) finish(v);
      });
      obs.observe(document.documentElement, { childList: true, subtree: true });
    });
  }

  function detectTheme() {
    try {
      const mode = document.documentElement.dataset
        ? document.documentElement.dataset.colorMode
        : null;
      if (mode === "dark") return "dark";
      if (mode === "light") return "light";
    } catch (_) { /* ignore */ }
    try {
      return window.matchMedia &&
        window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
    } catch (_) { return "light"; }
  }

  function pingPayload(owner, repo, stars) {
    const payload = { owner, repo };
    if (Number.isFinite(stars)) payload.stars = stars;
    return payload;
  }

  function pingFreshness(apiBase, owner, repo, stars) {
    try {
      const payload = pingPayload(owner, repo, stars);
      fetch(apiBase + "/api/ext/ping", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
        keepalive: true,
        credentials: "omit",
        referrerPolicy: "no-referrer"
      }).catch(() => {});
    } catch (_) { /* ignore */ }
  }

  // `enqueue=0` reads the same snapshot without offering queue work. Only
  // the first read of a repository needs to enqueue; the polls that follow
  // are waiting for that job, and re-offering it on every one of them is
  // what made one page visit dozens of enqueue-capable requests.
  async function fetchAnalyze(apiBase, owner, repo, signal, readOnly) {
    const url = apiBase + "/api/repos/" + encodeURIComponent(owner) + "/" +
      encodeURIComponent(repo) + "/analyze" + (readOnly ? "?enqueue=0" : "");
    const requestAbort = new AbortController();
    const abortFromRun = () => requestAbort.abort();
    let timedOut = false;
    if (signal) {
      if (signal.aborted) abortFromRun();
      else signal.addEventListener("abort", abortFromRun, { once: true });
    }
    const timeout = setTimeout(() => {
      timedOut = true;
      requestAbort.abort();
    }, ANALYZE_TIMEOUT_MS);
    try {
      const res = await fetch(url, {
        method: "GET", headers: { Accept: "application/json" },
        credentials: "omit",
        referrerPolicy: "no-referrer",
        signal: requestAbort.signal
      });
      if (!res.ok) throw new Error("analyze HTTP " + res.status);
      return await res.json();
    } catch (err) {
      if (timedOut) throw new Error("analyze timed out");
      throw err;
    } finally {
      clearTimeout(timeout);
      if (signal) signal.removeEventListener("abort", abortFromRun);
    }
  }

  function classifyAnalyze(data) {
    if (!data) return "pending";
    if (data.not_found === true) return "not_found";
    if (data.history_status === "not_public") return "not_found";
    if (data.history_status === "retrying" || data.history_unavailable === true) {
      return "retrying";
    }
    if (data.backfilling === true) return "backfilling";
    if (data.history_complete === true && data.pending !== true) return "ready";
    return "pending";
  }

  function chartUrl(apiBase, owner, repo, theme) {
    return apiBase + "/api/repos/" + encodeURIComponent(owner) + "/" +
      encodeURIComponent(repo) + "/chart.svg?theme=" + theme;
  }

  function statUrl(apiBase, owner, repo, name, theme) {
    return apiBase + "/api/repos/" + encodeURIComponent(owner) + "/" +
      encodeURIComponent(repo) + "/stats/" + name + ".svg?theme=" + theme;
  }

  function reportUrl(owner, repo) {
    return SITE_BASE + "/report?repo=" +
      encodeURIComponent(owner + "/" + repo) + "&ref=extension";
  }

  function compareUrl(owner, repo) {
    return SITE_BASE + "/compare?repos=" +
      encodeURIComponent(owner + "/" + repo) + "&ref=extension";
  }

  function createDisclosureController(toggle, panel, onFirstOpen, runtime) {
    const deps = runtime || {};
    const raf = deps.requestAnimationFrame ||
      ((callback) => window.requestAnimationFrame(callback));
    const cancelRaf = deps.cancelAnimationFrame ||
      ((id) => window.cancelAnimationFrame(id));
    const schedule = deps.setTimeout || setTimeout;
    const cancelSchedule = deps.clearTimeout || clearTimeout;
    const computedStyle = deps.getComputedStyle ||
      ((node) => window.getComputedStyle(node));
    const reduceMotion = deps.reduceMotion ||
      (() => {
        try {
          return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        } catch (_) {
          return false;
        }
      });

    let frameId = null;
    let fallbackId = null;
    let statsLoaded = false;

    function cancelPending() {
      if (frameId != null) {
        cancelRaf(frameId);
        frameId = null;
      }
      if (fallbackId != null) {
        cancelSchedule(fallbackId);
        fallbackId = null;
      }
    }

    function clearInlineMotion() {
      panel.style.removeProperty("opacity");
      panel.style.removeProperty("transform");
      panel.style.removeProperty("transition");
    }

    function settle(open) {
      cancelPending();
      clearInlineMotion();
      panel.dataset.state = open ? "open" : "closed";
      if (open) {
        panel.hidden = false;
        panel.removeAttribute("aria-hidden");
        panel.removeAttribute("inert");
      } else {
        panel.hidden = true;
        panel.setAttribute("aria-hidden", "true");
        panel.setAttribute("inert", "");
      }
    }

    function animate(open) {
      cancelPending();
      const wasHidden = panel.hidden;

      toggle.setAttribute("aria-expanded", String(open));
      if (open) {
        panel.hidden = false;
        panel.removeAttribute("aria-hidden");
        panel.removeAttribute("inert");
        if (!statsLoaded) {
          statsLoaded = true;
          onFirstOpen();
        }
      } else {
        panel.setAttribute("aria-hidden", "true");
        panel.setAttribute("inert", "");
      }

      const reduced = reduceMotion();
      const current = wasHidden ? null : computedStyle(panel);
      const startOpacity = current ? current.opacity : "0";
      const startTransform = reduced
        ? "none"
        : current && current.transform !== "none"
          ? current.transform
          : open
            ? "translateY(-4px) scale(0.98)"
            : "translateY(0) scale(1)";

      panel.dataset.state = open ? "opening" : "closing";
      panel.style.transition = "none";
      panel.style.opacity = startOpacity;
      panel.style.transform = startTransform;

      frameId = raf(() => {
        frameId = null;
        panel.dataset.state = open ? "open" : "closing";
        panel.style.transition = reduced
          ? "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1)"
          : open
            ? "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)"
            : "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)";
        panel.style.opacity = open ? "1" : "0";
        panel.style.transform = reduced
          ? "none"
          : open
            ? "translateY(0) scale(1)"
            : "translateY(-4px) scale(0.98)";
        fallbackId = schedule(() => settle(open), open ? 220 : 200);
      });
    }

    panel.addEventListener("transitionend", (event) => {
      if (event.target !== panel || event.propertyName !== "opacity") return;
      settle(toggle.getAttribute("aria-expanded") === "true");
    });

    return {
      toggle() {
        animate(toggle.getAttribute("aria-expanded") !== "true");
      },
      closeImmediately() {
        toggle.setAttribute("aria-expanded", "false");
        settle(false);
      }
    };
  }

  function buildPanel(ctx) {
    const host = el("div", { id: PANEL_ID });
    const shadow = host.attachShadow({ mode: "open" });
    shadow.appendChild(el("style", { text: PANEL_CSS }));

    const card = el("section", {
      class: "gd-card",
      "data-gd-theme": ctx.theme,
      role: "region",
      "aria-label": "gitdebt — star history & repo debt"
    });

    const title = el("h2", { class: "gd-title" }, [
      "Star history",
      el("span", {
        class: "gd-stars",
        title: "Stars",
        "aria-label": starAriaLabel(ctx.stars)
      }, [
        el("span", { class: "gd-star-icon", "aria-hidden": "true", text: "★" }),
        el("span", {
          class: "gd-star-count",
          "aria-hidden": "true",
          text: formatStars(ctx.stars)
        })
      ])
    ]);
    const brand = el("a", {
      class: "gd-brand", href: reportUrl(ctx.owner, ctx.repo),
      target: "_blank", rel: "noopener noreferrer", title: "Open full report on gitdebt"
    }, [
      el("img", {
        class: "gd-logo",
        src: ext.runtime.getURL("icons/icon-32.png"),
        alt: "",
        width: "20",
        height: "20"
      }),
      "gitdebt"
    ]);
    const header = el("div", { class: "gd-header" }, [title, brand]);

    const status = el("p", {
      class: "gd-status",
      role: "status",
      "aria-live": "polite",
      "aria-atomic": "true"
    }, "");
    const chartWrap = el("div", { class: "gd-chart-wrap" });

    const toggle = el("button", {
      class: "gd-toggle",
      type: "button",
      "aria-expanded": "false",
      "aria-controls": "gitdebt-stats",
      hidden: "hidden"
    }, [el("span", { class: "gd-toggle-label", text: "Repo-debt charts" }),
        el("span", { class: "gd-toggle-caret", "aria-hidden": "true", text: "▸" })]);
    const statsGrid = el("div", {
      id: "gitdebt-stats",
      class: "gd-stats-grid",
      "data-state": "closed",
      "aria-hidden": "true",
      inert: "",
      hidden: "hidden"
    });

    const report = el("a", {
      class: "gd-report", href: reportUrl(ctx.owner, ctx.repo),
      target: "_blank", rel: "noopener noreferrer",
      "aria-label": "Open the full gitdebt report in a new tab"
    }, ["Full report", el("span", { class: "gd-ext", "aria-hidden": "true", text: " ↗" })]);
    const compare = el("a", {
      class: "gd-report", href: compareUrl(ctx.owner, ctx.repo),
      target: "_blank", rel: "noopener noreferrer",
      title: "Overlay this repo's star history against others on gitdebt",
      "aria-label": "Compare this repository on gitdebt in a new tab"
    }, ["Compare", el("span", { class: "gd-ext", "aria-hidden": "true", text: " ↗" })]);
    const links = el("div", { class: "gd-links" }, [report, compare]);

    const body = el("div", { class: "gd-body" }, [status, chartWrap, toggle, statsGrid, links]);
    card.appendChild(header);
    card.appendChild(body);
    shadow.appendChild(card);

    const handles = {
      host, shadow, card, status, chartWrap, toggle, statsGrid,
      starCountEl: title.querySelector(".gd-star-count"),
      ctx
    };
    const disclosure = createDisclosureController(
      toggle,
      statsGrid,
      () => renderStatCharts(handles)
    );
    handles.disclosure = disclosure;
    toggle.addEventListener("click", disclosure.toggle);
    return handles;
  }

  function formatStars(n) {
    if (!Number.isFinite(n)) return "—";
    if (n >= 1000000) return (n / 1000000).toFixed(1).replace(/\.0$/, "") + "m";
    if (n >= 1000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "k";
    return String(n);
  }

  function starAriaLabel(n) {
    return Number.isFinite(n) ? formatStars(n) + " stars" : "Star count unavailable";
  }

  function setStatus(handles, text, kind) {
    const next = text || "";
    if (handles.status.textContent !== next) handles.status.textContent = next;
    handles.status.hidden = !text;
    handles.status.className = "gd-status" + (kind ? " gd-status-" + kind : "");
  }

  function renderStarChart(handles) {
    const ctx = handles.ctx;
    handles.chartWrap.textContent = "";
    const img = el("img", {
      class: "gd-chart", alt: "Star history for " + ctx.owner + "/" + ctx.repo,
      loading: "eager", decoding: "async",
      referrerpolicy: "no-referrer",
      src: chartUrl(ctx.apiBase, ctx.owner, ctx.repo, ctx.theme)
    });
    img.addEventListener("error", () => {
      handles.chartWrap.textContent = "";
      handles.chartWrap.appendChild(
        el("div", { class: "gd-chart-error", text: "The chart is not ready yet." })
      );
    });
    handles.chartWrap.appendChild(img);
    handles.toggle.hidden = false;
  }

  function renderStatCharts(handles) {
    const ctx = handles.ctx;
    handles.statsGrid.textContent = "";
    for (const stat of STAT_CHARTS) {
      const fig = el("figure", { class: "gd-stat" });
      const img = el("img", {
        class: "gd-stat-img", alt: stat.label + " for " + ctx.owner + "/" + ctx.repo,
        "data-stat-name": stat.name,
        loading: "lazy", decoding: "async",
        referrerpolicy: "no-referrer",
        src: statUrl(ctx.apiBase, ctx.owner, ctx.repo, stat.name, ctx.theme)
      });
      img.addEventListener("error", () => {
        fig.remove();
        if (!handles.statsGrid.querySelector(".gd-stat")) {
          handles.disclosure.closeImmediately();
          handles.toggle.hidden = true;
        }
      });
      fig.appendChild(img);
      fig.appendChild(el("figcaption", { class: "gd-stat-label", text: stat.label }));
      handles.statsGrid.appendChild(fig);
    }
  }

  function placeHost(host) {
    const sidebar = document.querySelector(".Layout-sidebar");
    if (sidebar && sidebar.offsetParent !== null) {
      host.setAttribute("data-gd-mode", "sidebar");
      const first = sidebar.firstElementChild;
      if (first && first.nextSibling) sidebar.insertBefore(host, first.nextSibling);
      else sidebar.appendChild(host);
      return true;
    }
    const main = document.querySelector(".Layout-main") ||
      document.querySelector("main") ||
      document.querySelector("#js-repo-pjax-container");
    if (main) {
      host.setAttribute("data-gd-mode", "main");
      main.insertBefore(host, main.firstChild);
      return true;
    }
    return false;
  }

  function ensurePlaced(host, token) {
    return new Promise((resolve) => {
      if (placeHost(host)) { resolve(true); return; }
      let done = false;
      const finish = (ok) => { if (done) return; done = true; obs.disconnect(); clearTimeout(timer); resolve(ok); };
      const obs = new MutationObserver(() => {
        if (token !== currentRunToken) { finish(false); return; }
        if (placeHost(host)) finish(true);
      });
      obs.observe(document.documentElement, { childList: true, subtree: true });
      const timer = setTimeout(() => finish(placeHost(host)), MOUNT_WAIT_MS);
    });
  }

  function removePanel() {
    const existing = document.getElementById(PANEL_ID);
    if (existing) existing.remove();
  }

  let currentRunToken = 0;
  let pollAbort = null;
  let activeHandles = null;
  let activeRepoKey = null;
  let lastPingedRepoKey = null;

  async function run() {
    const token = ++currentRunToken;
    if (pollAbort) { try { pollAbort.abort(); } catch (_) {} pollAbort = null; }
    activeHandles = null;
    removePanel();

    const repo = resolveRepo();
    const repoKey = repo
      ? repo.owner.toLowerCase() + "/" + repo.repo.toLowerCase()
      : null;
    activeRepoKey = repoKey;
    if (!repo) {
      lastPingedRepoKey = null;
      return;
    }
    if (!isPublicRepoPage()) {
      lastPingedRepoKey = null;
      return;
    }

    let settings;
    try { settings = await getSettings(); }
    catch (_) { settings = { enabled: true }; }
    if (token !== currentRunToken) return;
    if (!settings.enabled) return;

    const ctx = {
      apiBase: DEFAULT_API_BASE, owner: repo.owner, repo: repo.repo,
      stars: readStarCount(repo.owner, repo.repo), theme: detectTheme()
    };

    if (lastPingedRepoKey !== repoKey) {
      lastPingedRepoKey = repoKey;
      waitForStarCount(ctx.owner, ctx.repo, token, 3000).then((stars) => {
        if (token !== currentRunToken) return;
        if (Number.isFinite(stars)) ctx.stars = stars;
        pingFreshness(ctx.apiBase, ctx.owner, ctx.repo, ctx.stars);
      });
    }

    const handles = buildPanel(ctx);
    const placed = await ensurePlaced(handles.host, token);
    if (token !== currentRunToken) { handles.host.remove(); return; }
    if (!placed) { warnOnce("no-mount", "no mount point; skipping panel"); return; }
    activeHandles = handles;
    setStatus(handles, "Gathering data…", "loading");

    pollAbort = new AbortController();
    try {
      await analyzeAndRender(handles, token, pollAbort.signal);
    } catch (err) {
      if (token !== currentRunToken || (err && err.name === "AbortError")) return;
      warnOnce("analyze-fail", "backend unavailable:", err && err.message ? err.message : err);
      setStatus(handles, "gitdebt unavailable.", "error");
      renderStarChart(handles);
    }
  }

  async function analyzeAndRender(handles, token, signal) {
    let attempt = 0;
    let failures = 0;
    while (true) {
      if (token !== currentRunToken) return;
      let data;
      try {
        data = await fetchAnalyze(
          handles.ctx.apiBase,
          handles.ctx.owner,
          handles.ctx.repo,
          signal,
          attempt > 0
        );
        failures = 0;
      } catch (err) {
        if (err && err.name === "AbortError") throw err;
        failures += 1;
        if (failures >= 4) throw err;
        setStatus(handles, "Connection interrupted — retrying…", "loading");
        await sleep(Math.min(POLL_BASE_MS * failures, POLL_MAX_MS), signal);
        continue;
      }
      if (token !== currentRunToken) return;

      const cls = classifyAnalyze(data);

      if (cls === "not_found") {
        setStatus(handles, "Repository unavailable. It may be private or no longer exist.", "error");
        handles.toggle.hidden = true;
        return;
      }

      if (cls === "retrying") {
        setStatus(handles, "Star history retry scheduled — waiting for the data provider.", "loading");
        handles.toggle.hidden = false;
      }

      if (data && Number.isFinite(data.total_stars) && data.total_stars >= 0) {
        handles.starCountEl.textContent = formatStars(data.total_stars);
        handles.starCountEl.parentElement.setAttribute(
          "aria-label",
          starAriaLabel(data.total_stars)
        );
      }

      if (cls === "backfilling") {
        setStatus(handles, "Large repo — collecting a complete historical snapshot.", "loading");
        renderStarChart(handles);
        return;
      }

      if (cls === "ready") {
        setStatus(handles, "", null);
        renderStarChart(handles);
        return;
      }

      attempt += 1;
      if (attempt >= POLL_MAX_ATTEMPTS) {
        setStatus(handles, "Still gathering — open the full report to keep watching.", "loading");
        renderStarChart(handles);
        return;
      }
      setStatus(
        handles,
        cls === "retrying"
          ? "Star history retry scheduled — waiting for the data provider."
          : "Gathering data… (working)",
        "loading"
      );
      await sleep(Math.min(POLL_BASE_MS + attempt * 500, POLL_MAX_MS), signal);
      // A background tab is not showing the panel; wait for it to come back
      // rather than polling from every tab a user left open.
      while (document.hidden) {
        await sleep(POLL_MAX_MS, signal);
      }
    }
  }

  function sleep(ms, signal) {
    return new Promise((resolve, reject) => {
      if (signal && signal.aborted) {
        const e = new Error("aborted");
        e.name = "AbortError";
        reject(e);
        return;
      }
      const onAbort = () => {
        clearTimeout(timer);
        const e = new Error("aborted");
        e.name = "AbortError";
        reject(e);
      };
      const timer = setTimeout(() => {
        if (signal) signal.removeEventListener("abort", onAbort);
        resolve();
      }, ms);
      if (signal) signal.addEventListener("abort", onAbort, { once: true });
    });
  }

  function refreshTheme() {
    if (!activeHandles) return;
    const nextTheme = detectTheme();
    if (nextTheme === activeHandles.ctx.theme) return;
    activeHandles.ctx.theme = nextTheme;
    activeHandles.card.setAttribute("data-gd-theme", nextTheme);
    const chart = activeHandles.shadow.querySelector(".gd-chart");
    if (chart) {
      chart.src = chartUrl(
        activeHandles.ctx.apiBase,
        activeHandles.ctx.owner,
        activeHandles.ctx.repo,
        nextTheme
      );
    }
    for (const img of activeHandles.shadow.querySelectorAll(".gd-stat-img")) {
      img.src = statUrl(
        activeHandles.ctx.apiBase,
        activeHandles.ctx.owner,
        activeHandles.ctx.repo,
        img.dataset.statName,
        nextTheme
      );
    }
  }

  let lastUrl = location.href;

  function maybeRun() {
    if (maybeRun._t) clearTimeout(maybeRun._t);
    maybeRun._t = setTimeout(() => {
      lastUrl = location.href;
      const next = resolveRepo();
      const nextKey = next
        ? next.owner.toLowerCase() + "/" + next.repo.toLowerCase()
        : null;
      if (nextKey === activeRepoKey && document.getElementById(PANEL_ID)) return;
      if (nextKey === null && activeRepoKey === null) return;
      run().catch((e) => warnOnce("run-fail", "run failed:", e && e.message));
    }, 80);
  }

  function onUrlMaybeChanged() {
    if (location.href !== lastUrl) maybeRun();
  }

  document.addEventListener("turbo:load", maybeRun);
  document.addEventListener("turbo:render", maybeRun);
  document.addEventListener("pjax:end", maybeRun);
  document.addEventListener("pjax:success", maybeRun);
  window.addEventListener("popstate", onUrlMaybeChanged);
  try {
    ext.storage.onChanged.addListener((changes, areaName) => {
      if (areaName === "sync" && changes.enabled) {
        run().catch((e) => warnOnce("settings-run-fail", "settings update failed:", e && e.message));
      }
    });
  } catch (_) {}
  const headObserver = new MutationObserver(onUrlMaybeChanged);
  headObserver.observe(document.head, {
    childList: true,
    subtree: true,
    characterData: true
  });
  const themeObserver = new MutationObserver(refreshTheme);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-color-mode", "data-light-theme", "data-dark-theme"]
  });
  try {
    window.matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", refreshTheme);
  } catch (_) {}

  maybeRun();

  const PANEL_CSS = `
:host { all: initial; display: block; }
* { box-sizing: border-box; }

.gd-card {
  --gd-bg: var(--bgColor-default, var(--color-canvas-default, #ffffff));
  --gd-subtle: var(--bgColor-muted, var(--color-canvas-subtle, #f6f8fa));
  --gd-fg: var(--fgColor-default, var(--color-fg-default, #1f2328));
  --gd-muted: var(--fgColor-muted, var(--color-fg-muted, #59636e));
  --gd-border: var(--borderColor-default, var(--color-border-default, #d1d9e0));
  --gd-accent: #5b2cff;
  --gd-gold: var(--fgColor-attention, var(--color-attention-fg, #9a6700));
  --gd-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
  --gd-field: url("data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='8'%20height='8'%3E%3Cpath%20fill='%235b2cff'%20fill-opacity='.5'%20d='M0%200h2v2H0zM4%200h2v2H4zM2%202h2v2H2zM0%204h2v2H0zM4%204h2v2H4zM6%206h2v2H6z'/%3E%3Cpath%20fill='%235b2cff'%20fill-opacity='.2'%20d='M2%200h2v2H2zM6%200h2v2H6zM0%202h2v2H0zM4%202h2v2H4zM6%202h2v2H6zM2%204h2v2H2zM6%204h2v2H6zM0%206h2v2H0zM2%206h2v2H2zM4%206h2v2H4z'/%3E%3C/svg%3E");
  --gd-field-alpha: 0.22;

  position: relative;
  isolation: isolate;
  font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif;
  color: var(--gd-fg);
  background: var(--gd-bg);
  border: 1px solid var(--gd-border);
  border-radius: 6px;
  overflow: hidden;
}
.gd-card[data-gd-theme="dark"] {
  --gd-accent: #9b7bff;
  --gd-field: url("data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='8'%20height='8'%3E%3Cpath%20fill='%239b7bff'%20fill-opacity='.5'%20d='M0%200h2v2H0zM4%200h2v2H4zM2%202h2v2H2zM0%204h2v2H0zM4%204h2v2H4zM6%206h2v2H6z'/%3E%3Cpath%20fill='%239b7bff'%20fill-opacity='.2'%20d='M2%200h2v2H2zM6%200h2v2H6zM0%202h2v2H0zM4%202h2v2H4zM6%202h2v2H6zM2%204h2v2H2zM6%204h2v2H6zM0%206h2v2H0zM2%206h2v2H2zM4%206h2v2H4z'/%3E%3C/svg%3E");
  --gd-field-alpha: 0.3;
}

.gd-header {
  position: relative;
  display: flex; align-items: center; justify-content: space-between; gap: 8px;
  padding: 8px 16px; border-bottom: 1px solid var(--gd-border);
}
.gd-header::before {
  content: ""; position: absolute; inset: 0; z-index: -1;
  background-image: var(--gd-field);
  opacity: var(--gd-field-alpha);
  -webkit-mask-image: linear-gradient(to left, #000, transparent 70%);
  mask-image: linear-gradient(to left, #000, transparent 70%);
  pointer-events: none;
}
.gd-title {
  display: flex; align-items: center; gap: 8px;
  margin: 0; font: 600 11px/1.5 var(--gd-mono);
  letter-spacing: 0.08em; text-transform: uppercase; color: var(--gd-fg);
}
.gd-stars {
  display: inline-flex; align-items: center; gap: 4px;
  font: 500 11px/1.5 var(--gd-mono); letter-spacing: 0.02em;
  color: var(--gd-muted);
}
.gd-star-icon { color: var(--gd-gold); font-size: 11px; line-height: 1; }
.gd-star-count { font-variant-numeric: tabular-nums; }

.gd-brand {
  display: inline-flex; align-items: center; gap: 6px;
  font: 600 12px/1.5 var(--gd-mono); letter-spacing: -0.02em;
  text-transform: none;
  color: var(--gd-muted); text-decoration: none;
}
.gd-logo { display: block; width: 20px; height: 20px; }
.gd-brand:hover { color: var(--gd-fg); }
.gd-brand:focus-visible, .gd-report:focus-visible, .gd-toggle:focus-visible {
  outline: 2px solid var(--gd-accent); outline-offset: 2px;
}

.gd-body { padding: 12px 16px 14px; }

.gd-status {
  display: flex; align-items: center; gap: 8px;
  margin: 0 0 10px; font: 11px/1.6 var(--gd-mono);
  letter-spacing: 0.02em; color: var(--gd-muted);
}
.gd-status[hidden] { display: none; }
.gd-status-loading::before {
  content: ""; width: 9px; height: 9px; border-radius: 50%;
  background: var(--gd-accent); flex: 0 0 auto;
  animation: gd-pulse 1.4s ease-out infinite;
}
@keyframes gd-pulse {
  0% { box-shadow: 0 0 0 0 color-mix(in oklab, var(--gd-accent) 50%, transparent); }
  70% { box-shadow: 0 0 0 7px transparent; } 100% { box-shadow: 0 0 0 0 transparent; }
}
@media (prefers-reduced-motion: reduce) { .gd-status-loading::before { animation: none; } }
.gd-status-error { color: var(--fgColor-danger, var(--color-danger-fg, #cf222e)); }

.gd-chart-wrap { width: 100%; }
.gd-chart {
  display: block; width: 100%; height: auto; border-radius: 6px;
  border: 1px solid var(--gd-border); background: var(--gd-bg);
}
.gd-chart-error {
  padding: 16px 12px; text-align: center;
  font: 11px/1.6 var(--gd-mono); letter-spacing: 0.02em; color: var(--gd-muted);
  border: 1px dashed var(--gd-border); border-radius: 6px;
}

.gd-toggle {
  appearance: none; -webkit-appearance: none;
  display: flex; align-items: center; justify-content: space-between; gap: 6px;
  width: 100%; min-height: 32px; margin-top: 12px; padding: 6px 10px;
  font: 600 10px/1.6 var(--gd-mono);
  letter-spacing: 0.08em; text-transform: uppercase;
  color: var(--gd-fg);
  background: color-mix(in srgb, var(--gd-accent) 4%, var(--gd-subtle));
  border: 1px solid var(--gd-border); border-radius: 6px; cursor: pointer;
}
.gd-toggle[hidden] { display: none; }
.gd-toggle:hover {
  border-color: color-mix(in srgb, var(--gd-accent) 55%, var(--gd-border));
}
.gd-toggle-caret {
  display: inline-block; color: var(--gd-muted); font-size: 10px;
  transform: rotate(0deg);
  transition: transform 160ms cubic-bezier(0.77, 0, 0.175, 1);
}
.gd-toggle[aria-expanded="true"] .gd-toggle-caret { transform: rotate(90deg); }

.gd-stats-grid {
  display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px; margin-top: 10px; transform-origin: top center;
}
.gd-stats-grid[hidden] { display: none; }
.gd-stats-grid[data-state="closed"],
.gd-stats-grid[data-state="opening"],
.gd-stats-grid[data-state="closing"] {
  opacity: 0; transform: translateY(-4px) scale(0.98);
}
.gd-stats-grid[data-state="open"] {
  opacity: 1; transform: translateY(0) scale(1);
  transition:
    opacity 180ms cubic-bezier(0.23, 1, 0.32, 1),
    transform 180ms cubic-bezier(0.23, 1, 0.32, 1);
}
.gd-stats-grid[data-state="closing"] {
  transition:
    opacity 140ms cubic-bezier(0.23, 1, 0.32, 1),
    transform 140ms cubic-bezier(0.23, 1, 0.32, 1);
}
.gd-stat {
  margin: 0; border: 1px solid var(--gd-border); border-radius: 6px;
  overflow: hidden; background: var(--gd-bg);
}
.gd-stat-img { display: block; width: 100%; height: auto; }
.gd-stat-label {
  font: 10px/1.7 var(--gd-mono); letter-spacing: 0.06em; text-transform: uppercase;
  color: var(--gd-muted); padding: 4px 8px;
  border-top: 1px solid var(--gd-border);
}

.gd-links { display: flex; align-items: center; gap: 14px; margin-top: 12px; }
.gd-report {
  display: inline-block;
  font: 600 11px/1.6 var(--gd-mono);
  letter-spacing: 0.06em; text-transform: uppercase;
  text-decoration: none; color: var(--gd-accent);
}
.gd-report:hover { text-decoration: underline; }
.gd-ext { font-weight: 600; }

@media (prefers-reduced-motion: reduce) {
  .gd-toggle-caret { transition: none; }
  .gd-stats-grid[data-state] { transform: none; }
  .gd-stats-grid[data-state="open"] {
    transition: opacity 140ms cubic-bezier(0.23, 1, 0.32, 1);
  }
  .gd-stats-grid[data-state="closing"] {
    transition: opacity 140ms cubic-bezier(0.23, 1, 0.32, 1);
  }
}
`;
})();
