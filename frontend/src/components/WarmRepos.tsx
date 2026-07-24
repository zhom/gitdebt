import { useEffect } from "react";

const SLUG_RE = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;

type Lane = "stars" | "history";
type Task = {
  apiBase: string;
  attempts: number;
  lane: Lane;
  slug: string;
};

const queues: Record<Lane, Task[]> = {
  stars: [],
  history: [],
};
const active: Record<Lane, boolean> = {
  stars: false,
  history: false,
};
const queued = new Set<string>();
const completed = new Set<string>();
const lastStarted: Record<Lane, number> = {
  stars: 0,
  history: 0,
};

const INTERVAL_MS: Record<Lane, number> = {
  stars: 525,
  history: 1_050,
};

function taskKey(task: Pick<Task, "apiBase" | "lane" | "slug">): string {
  return `${task.apiBase}|${task.lane}|${task.slug}`;
}

function normalizeRepos(repos: readonly string[]): string[] {
  return Array.from(
    new Set(
      repos
        .map((slug) => slug.trim().toLowerCase())
        .filter((slug) => SLUG_RE.test(slug)),
    ),
  );
}

function prioritizeQueuedTask(key: string, lane: Lane) {
  const index = queues[lane].findIndex((task) => taskKey(task) === key);
  if (index <= 0) return;
  const [task] = queues[lane].splice(index, 1);
  queues[lane].unshift(task);
}

function enqueueTask(task: Task, priority: boolean) {
  const key = taskKey(task);
  if (completed.has(key)) return;
  if (queued.has(key)) {
    if (priority) prioritizeQueuedTask(key, task.lane);
    return;
  }
  queued.add(key);
  if (priority) queues[task.lane].unshift(task);
  else queues[task.lane].push(task);
  void pump(task.lane);
}

function retryTask(task: Task, key: string) {
  queued.delete(key);
  enqueueTask({ ...task, attempts: task.attempts + 1 }, false);
}

async function waitForLane(lane: Lane) {
  const remaining =
    lastStarted[lane] + INTERVAL_MS[lane] - performance.now();
  if (remaining <= 0) return;
  await new Promise((resolve) => setTimeout(resolve, remaining));
}

async function runTask(task: Task): Promise<Response> {
  const path =
    task.lane === "stars"
      ? `/api/repos/${task.slug}/analyze`
      : `/api/repos/${task.slug}/analyze-history`;
  return fetch(`${task.apiBase}${path}`, {
    method: task.lane === "stars" ? "GET" : "POST",
    cache: "no-store",
    credentials: "omit",
    headers: { accept: "application/json" },
    keepalive: true,
    signal: AbortSignal.timeout(8_000),
  });
}

async function pump(lane: Lane) {
  if (active[lane]) return;
  active[lane] = true;
  try {
    while (queues[lane].length > 0) {
      await waitForLane(lane);
      const task = queues[lane].shift();
      if (!task) continue;
      const key = taskKey(task);
      lastStarted[lane] = performance.now();
      try {
        const response = await runTask(task);
        await response.arrayBuffer();
        // 429 is deliberately absent: a rate limiter's answer is "stop",
        // not "try again immediately". Retrying it turned every limited
        // warm-up into three requests instead of one.
        const retryable =
          response.status === 408 ||
          response.status === 425 ||
          response.status >= 500;
        if (retryable && task.attempts < 2) {
          retryTask(task, key);
        } else {
          completed.add(key);
          queued.delete(key);
        }
      } catch {
        if (task.attempts < 2) retryTask(task, key);
        else queued.delete(key);
      }
    }
  } finally {
    active[lane] = false;
    if (queues[lane].length > 0) void pump(lane);
  }
}

export function warmRepos(
  apiBase: string,
  repos: readonly string[],
  priority = false,
) {
  for (const slug of normalizeRepos(repos)) {
    enqueueTask({ apiBase, attempts: 0, lane: "stars", slug }, priority);
    enqueueTask({ apiBase, attempts: 0, lane: "history", slug }, priority);
  }
}

function reposFromTarget(target: EventTarget | null): string[] {
  if (!(target instanceof Element)) return [];
  const link = target.closest<HTMLElement>("[data-warm-repos]");
  return link?.dataset.warmRepos?.split(",") ?? [];
}

export function WarmRepos({
  apiBase,
  repos,
}: {
  apiBase: string;
  repos: string[];
}) {
  useEffect(() => {
    const allowed = new Set(normalizeRepos(repos));

    function prioritize(event: Event) {
      const targeted = reposFromTarget(event.target).filter((repo) =>
        allowed.has(repo.toLowerCase()),
      );
      if (targeted.length > 0) warmRepos(apiBase, targeted, true);
    }

    document.addEventListener("pointerover", prioritize, { passive: true });
    document.addEventListener("focusin", prioritize);
    document.addEventListener("touchstart", prioritize, { passive: true });
    return () => {
      document.removeEventListener("pointerover", prioritize);
      document.removeEventListener("focusin", prioritize);
      document.removeEventListener("touchstart", prioritize);
    };
  }, [apiBase, repos]);

  return null;
}
