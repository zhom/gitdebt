(() => {
  "use strict";

  const ext = globalThis.chrome || globalThis.browser;
  const SITE_BASE = "https://gitdebt.com";
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

  const enabledEl = document.getElementById("gd-enabled");
  const feedbackEl = document.getElementById("gd-feedback");
  const openBtn = document.getElementById("gd-open");
  const openHint = document.getElementById("gd-open-hint");
  const versionEl = document.getElementById("gd-version");
  let currentRepoUrl = null;

  function setFeedback(message) {
    feedbackEl.textContent = message || "";
    feedbackEl.hidden = !message;
  }

  function resolveRepoFromUrl(rawUrl) {
    try {
      const url = new URL(rawUrl);
      if (url.hostname !== "github.com") return null;
      const parts = url.pathname.split("/").filter(Boolean);
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

  function loadEnabled() {
    try {
      ext.storage.sync.get({ enabled: true }, (items) => {
        if (ext.runtime && ext.runtime.lastError) {
          enabledEl.checked = true;
          setFeedback("Could not load the saved setting.");
          return;
        }
        enabledEl.checked = !items || items.enabled !== false;
      });
    } catch (_) {
      enabledEl.checked = true;
      setFeedback("Could not load the saved setting.");
    }
  }

  function saveEnabled(enabled) {
    return new Promise((resolve) => {
      try {
        ext.storage.sync.set({ enabled }, () => {
          const failed = Boolean(ext.runtime && ext.runtime.lastError);
          resolve(!failed);
        });
      } catch (_) {
        resolve(false);
      }
    });
  }

  async function toggleEnabled() {
    setFeedback("");
    const next = enabledEl.checked;
    if (await saveEnabled(next)) return;
    enabledEl.checked = !next;
    setFeedback("Could not save the setting.");
  }

  function detectActiveRepo() {
    try {
      ext.tabs.query({ active: true, currentWindow: true }, (tabs) => {
        if (ext.runtime && ext.runtime.lastError) {
          openHint.textContent = "Could not read the active tab.";
          return;
        }
        const tab = tabs && tabs[0];
        const repo = tab && tab.url ? resolveRepoFromUrl(tab.url) : null;
        if (!repo) return;
        ext.tabs.sendMessage(
          tab.id,
          { type: "gitdebt:get-repo-context" },
          (context) => {
            if (ext.runtime && ext.runtime.lastError) {
              openHint.textContent =
                "Reload this repository to enable its gitdebt report.";
              return;
            }
            if (!context || context.isPublic !== true) {
              openHint.textContent =
                "Reports are available for public repositories only.";
              return;
            }
            currentRepoUrl =
              SITE_BASE + "/report?repo=" +
              encodeURIComponent(repo.owner + "/" + repo.repo) +
              "&ref=extension";
            openBtn.disabled = false;
            openBtn.textContent = "Open " + repo.owner + "/" + repo.repo;
            openBtn.setAttribute(
              "aria-label",
              "Open " + repo.owner + "/" + repo.repo +
              " on gitdebt in a new tab"
            );
            openHint.hidden = true;
          }
        );
      });
    } catch (_) {
      openHint.textContent = "Could not read the active tab.";
    }
  }

  function openCurrentRepo() {
    if (!currentRepoUrl) return;
    try {
      ext.tabs.create({ url: currentRepoUrl });
    } catch (_) {
      window.open(currentRepoUrl, "_blank", "noopener");
    }
  }

  enabledEl.addEventListener("change", toggleEnabled);
  openBtn.addEventListener("click", openCurrentRepo);

  try {
    versionEl.textContent = "v" + ext.runtime.getManifest().version;
  } catch (_) {
    versionEl.hidden = true;
  }
  loadEnabled();
  detectActiveRepo();
})();
