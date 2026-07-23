/**
 * Curated comparison categories — the source of truth for the
 * programmatic /compare/{category} pages.
 *
 * Editorial rules (from the SEO spec):
 * - Quality over quantity: a small, deliberate set of categories with
 *   4–8 genuinely popular member repos each. Never auto-generate more.
 * - `intro` is HAND-WRITTEN per category (unique crawlable text — the
 *   anti-thin-page requirement). Do not template it.
 * - Member slugs are lowercase `owner/repo` (the cache + CDN convention).
 * - Framing is celebratory and factual. Rankings are of repos by public
 *   star timestamps — never anything about accounts.
 *
 * The array order is semantic: neighboring categories are topically
 * close, so `relatedCategories` (prev/next neighbors) yields sensible
 * cross-links without a separate curation table.
 */

export type Category = {
  /** URL slug under /compare/. Lowercase, hyphenated. */
  slug: string;
  /** Display name, e.g. "React frameworks". */
  name: string;
  /** One-line summary for hubs, link lists, and meta descriptions. */
  short: string;
  /** Hand-written intro paragraph. Unique per category — never templated. */
  intro: string;
  /** Ordered member repos (owner/repo, lowercase). 4–8 entries. */
  repos: string[];
};

export const CATEGORIES: Category[] = [
  {
    slug: "frontend-frameworks",
    name: "Frontend frameworks",
    short: "React, Vue, Svelte, Angular, Solid and Preact head to head.",
    intro:
      "Frontend frameworks are the longest-running rivalry on GitHub. React's curve is the benchmark every newcomer gets measured against, Vue's climb through the late 2010s was the fastest the ecosystem had seen, and Svelte and Solid show what a post-virtual-DOM pitch does to a growth curve. The points where the lines cross tend to line up with real shifts in adoption.",
    repos: [
      "facebook/react",
      "vuejs/vue",
      "sveltejs/svelte",
      "angular/angular",
      "solidjs/solid",
      "preactjs/preact",
    ],
  },
  {
    slug: "react-frameworks",
    name: "React meta-frameworks",
    short: "Next.js, React Router, Gatsby, Redwood and Expo compared.",
    intro:
      "Picking React is the easy part; picking how to ship it is where teams argue. Next.js has pulled far ahead on stars, but the shape of each curve matters more than the totals: Gatsby's plateau marks the end of the static-site-only era, React Router's steady line reflects a decade as the default routing layer, and Expo's climb tracks React Native going mainstream. Compare the trajectories before you commit a codebase to one.",
    repos: [
      "vercel/next.js",
      "remix-run/react-router",
      "gatsbyjs/gatsby",
      "redwoodjs/redwood",
      "expo/expo",
    ],
  },
  {
    slug: "static-site-generators",
    name: "Static site generators",
    short: "Astro, Hugo, Eleventy, Jekyll, Zola and Hexo growth compared.",
    intro:
      "Static site generators are where web tooling fashions show up first. Jekyll's early curve is a fossil record of the GitHub Pages era, Hugo's steady grind proves single-binary speed never goes out of style, and Astro's near-vertical takeoff is what happens when a project catches the islands-architecture moment perfectly. If you want to see a tooling generation change hands in one chart, this is the category.",
    repos: [
      "withastro/astro",
      "gohugoio/hugo",
      "11ty/eleventy",
      "jekyll/jekyll",
      "getzola/zola",
      "hexojs/hexo",
    ],
  },
  {
    slug: "css-frameworks",
    name: "CSS frameworks",
    short: "Tailwind CSS, Bootstrap, Bulma, UnoCSS and Pico compared.",
    intro:
      "Bootstrap owned an entire decade of the web, and its star history shows it — a curve so early and so large that everything since gets judged against it. Tailwind's utility-first bet looked contrarian in 2019 and inevitable by 2022; the crossover between those two lines is one of the cleanest changing-of-the-guard moments on GitHub. Bulma, UnoCSS and Pico each carve out real niches in the gaps.",
    repos: [
      "tailwindlabs/tailwindcss",
      "twbs/bootstrap",
      "jgthms/bulma",
      "unocss/unocss",
      "picocss/pico",
    ],
  },
  {
    slug: "build-tools",
    name: "JavaScript build tools",
    short: "Vite, webpack, esbuild, Rollup, SWC and Oxc build speed rivals.",
    intro:
      "Few categories have flipped as fast as JavaScript bundlers. webpack spent years as the unquestioned default, then esbuild proved builds could be two orders of magnitude faster, and Vite packaged that speed into an experience developers actually enjoyed — its star curve since 2021 is among the steepest of any devtool. SWC and Oxc are the Rust-powered next wave; watch their slopes, not their totals.",
    repos: [
      "vitejs/vite",
      "webpack/webpack",
      "evanw/esbuild",
      "rollup/rollup",
      "swc-project/swc",
      "oxc-project/oxc",
    ],
  },
  {
    slug: "js-runtimes",
    name: "JavaScript runtimes",
    short: "Node.js, Deno and Bun — the server-side JS race.",
    intro:
      "Node.js normalized JavaScript on the server and spent a decade unchallenged. Then Deno arrived with a security-first pitch from Node's own creator, and Bun's first months of stars rank among the fastest climbs GitHub has recorded for infrastructure software. The totals still favor the incumbent; the recent-velocity numbers are a much closer race.",
    repos: [
      "nodejs/node",
      "denoland/deno",
      "oven-sh/bun",
      "quickjs-ng/quickjs",
    ],
  },
  {
    slug: "orms-typescript",
    name: "TypeScript ORMs",
    short: "Prisma, Drizzle, TypeORM, Sequelize, MikroORM and Knex.",
    intro:
      "Every generation of Node backend rediscovers the database layer. Sequelize and Knex carried the callback and promise eras, TypeORM rode the decorator wave, Prisma turned schema-first tooling into a product, and Drizzle's SQL-flavored minimalism is the current fast riser. The star curves here map almost perfectly onto how TypeScript itself matured — compare Drizzle's slope against Prisma's to see the current argument in one picture.",
    repos: [
      "prisma/prisma",
      "drizzle-team/drizzle-orm",
      "typeorm/typeorm",
      "sequelize/sequelize",
      "mikro-orm/mikro-orm",
      "knex/knex",
    ],
  },
  {
    slug: "python-web-frameworks",
    name: "Python web frameworks",
    short: "Django, Flask, FastAPI, Starlette, Litestar and Tornado.",
    intro:
      "Django and Flask split the Python web for a decade — batteries-included versus micro — until FastAPI arrived and put async, type hints, and automatic OpenAPI docs into one package. FastAPI reaching parity with frameworks fifteen years its senior is one of the defining star-history stories in Python. Starlette (the engine under FastAPI) and Litestar are worth watching if you want to see where the async stack goes next.",
    repos: [
      "django/django",
      "pallets/flask",
      "fastapi/fastapi",
      "encode/starlette",
      "litestar-org/litestar",
      "tornadoweb/tornado",
    ],
  },
  {
    slug: "python-data-tools",
    name: "Python data tools",
    short: "pandas, Polars, NumPy, Dask and Arrow for data crunching.",
    intro:
      "NumPy and pandas are the bedrock nearly every data stack is built on, which makes their long, steady star curves a useful baseline: this is what indispensable looks like. Polars is the disruption story — a Rust-core DataFrame library whose growth since 2022 tracks the whole ecosystem's hunger for speed. Arrow and Dask fill in the picture at the memory-format and distributed ends.",
    repos: [
      "pandas-dev/pandas",
      "pola-rs/polars",
      "numpy/numpy",
      "dask/dask",
      "apache/arrow",
    ],
  },
  {
    slug: "rust-web-frameworks",
    name: "Rust web frameworks",
    short: "Axum, Actix Web, Rocket, warp, Poem and Salvo compared.",
    intro:
      "Rust web frameworks are a study in ecosystem consolidation. Actix Web's benchmark dominance made it the early flagship, Rocket bet on ergonomics before the async ecosystem was ready, and Axum — riding the Tokio team's credibility — became the community default almost as soon as it shipped. The relative slopes since 2021 show that shift clearly, and they matter more than any single benchmark table.",
    repos: [
      "tokio-rs/axum",
      "actix/actix-web",
      "rwf2/rocket",
      "seanmonstar/warp",
      "poem-web/poem",
      "salvo-rs/salvo",
    ],
  },
  {
    slug: "code-editors",
    name: "Code editors",
    short: "VS Code, Neovim, Vim, Zed and Helix — editor star history.",
    intro:
      "VS Code is the biggest open-source editor project ever measured by stars, but the more interesting lines are underneath it: Neovim decisively outgrowing the Vim it forked from, Helix proving a modal editor can be born modern, and Zed converting years of anticipation into one of the sharpest launch spikes in the category's history.",
    repos: [
      "microsoft/vscode",
      "neovim/neovim",
      "vim/vim",
      "zed-industries/zed",
      "helix-editor/helix",
    ],
  },
  {
    slug: "terminal-tools",
    name: "Terminals & multiplexers",
    short: "Alacritty, kitty, WezTerm, Ghostty, Zellij and tmux.",
    intro:
      "Terminal emulators picked up a new generation of projects once GPU acceleration arrived. Alacritty and kitty kicked off that era, WezTerm quietly built a power-user following, and Ghostty's 2024 release turned years of private beta into an instant vertical line. On the multiplexer side, tmux's decade-long slow burn against Zellij's newer curve is a portrait of stability versus reinvention.",
    repos: [
      "alacritty/alacritty",
      "kovidgoyal/kitty",
      "wez/wezterm",
      "ghostty-org/ghostty",
      "zellij-org/zellij",
      "tmux/tmux",
      "microsoft/terminal",
    ],
  },
  {
    slug: "ai-coding-tools",
    name: "AI coding tools",
    short: "Aider, Cline, Continue, OpenHands, Gemini CLI and Codex.",
    intro:
      "No category on GitHub has grown faster in the 2020s than open-source AI coding tools. Aider made terminal-native pair programming credible, Cline and Continue brought agents into the editor, and OpenHands pushed toward fully autonomous software work — each with a star curve that would have led any other category's chart. When the model labs themselves started shipping open CLIs, the growth only compounded. Recent velocity matters more than totals here; leaders change by the quarter.",
    repos: [
      "aider-ai/aider",
      "cline/cline",
      "continuedev/continue",
      "all-hands-ai/openhands",
      "google-gemini/gemini-cli",
      "openai/codex",
    ],
  },
  {
    slug: "llm-inference",
    name: "LLM inference engines",
    short: "llama.cpp, vLLM, Ollama, SGLang and TGI serving stacks.",
    intro:
      "Running large language models yourself went from research stunt to commodity in about two years, and these five projects did most of the commoditizing. llama.cpp proved frontier-class models could run on a laptop, Ollama wrapped that power in a one-line install, and vLLM and SGLang turned GPU serving throughput into a public leaderboard of its own. The star histories here are effectively a timeline of the local-AI movement.",
    repos: [
      "ggml-org/llama.cpp",
      "vllm-project/vllm",
      "ollama/ollama",
      "sgl-project/sglang",
      "huggingface/text-generation-inference",
    ],
  },
  {
    slug: "vector-databases",
    name: "Vector databases",
    short: "Qdrant, Milvus, Weaviate, Chroma, FAISS and pgvector.",
    intro:
      "Vector search jumped from an information-retrieval niche to core infrastructure the moment retrieval-augmented generation became the default LLM architecture. The 2022–2023 inflection is visible in every curve in this category — Chroma sprinted from zero alongside the LangChain wave, Qdrant and Weaviate converted the moment into durable growth, and pgvector made the counter-argument that your existing Postgres was the vector database all along.",
    repos: [
      "qdrant/qdrant",
      "milvus-io/milvus",
      "weaviate/weaviate",
      "chroma-core/chroma",
      "facebookresearch/faiss",
      "pgvector/pgvector",
    ],
  },
  {
    slug: "sql-databases",
    name: "Open-source SQL databases",
    short: "Postgres, MySQL, SQLite, MariaDB, CockroachDB, DuckDB, ClickHouse.",
    intro:
      "Databases age differently than frameworks: the incumbents here measure their histories in decades, so their star curves under-count influence enormously — Postgres and SQLite run half the world from mirrors that only joined GitHub late. That makes the newer entrants easier to read: DuckDB's analytics-in-process pitch and ClickHouse's columnar speed both show the sharp modern growth you'd expect from projects born on GitHub.",
    repos: [
      "postgres/postgres",
      "mysql/mysql-server",
      "sqlite/sqlite",
      "mariadb/server",
      "cockroachdb/cockroach",
      "duckdb/duckdb",
      "clickhouse/clickhouse",
    ],
  },
  {
    slug: "message-queues",
    name: "Message queues & streaming",
    short: "Kafka, RabbitMQ, NATS, Pulsar and Redpanda compared.",
    intro:
      "Event streaming quietly became the backbone of modern backends, and this category shows the full spectrum: Kafka's long institutional climb, RabbitMQ's steady reign as the pragmatic default, NATS growing on the strength of being small and fast, and Redpanda pitching Kafka compatibility without the JVM. These are infrastructure curves — slower and steadier than devtools, which makes any sudden slope change worth investigating.",
    repos: [
      "apache/kafka",
      "rabbitmq/rabbitmq-server",
      "nats-io/nats-server",
      "apache/pulsar",
      "redpanda-data/redpanda",
    ],
  },
  {
    slug: "container-platforms",
    name: "Container platforms",
    short: "Kubernetes, Compose, k3s, Nomad, Rancher and Podman.",
    intro:
      "Kubernetes won container orchestration so thoroughly that the interesting comparisons are now within its own ecosystem: k3s made the case for small clusters, Rancher for manageable ones. Around the edges, Docker Compose remains the tool developers actually reach for daily, Podman built a daemonless following, and Nomad kept a loyal base by refusing to be Kubernetes. Contrasting Compose's developer-tool curve against Kubernetes' platform curve is instructive.",
    repos: [
      "kubernetes/kubernetes",
      "docker/compose",
      "k3s-io/k3s",
      "hashicorp/nomad",
      "rancher/rancher",
      "containers/podman",
    ],
  },
  {
    slug: "observability",
    name: "Monitoring & observability",
    short: "Grafana, Prometheus, Netdata, SigNoz, VictoriaMetrics, OTel.",
    intro:
      "Observability is where open source beat the commercial incumbents in plain sight. Grafana and Prometheus grew into the default dashboard-and-metrics pair for a generation of infrastructure, Netdata took the single-node niche with zero-config appeal, and SigNoz and VictoriaMetrics are the newer curves betting on all-in-one tracing and raw efficiency respectively. The OpenTelemetry collector's rise tracks the whole industry converging on one wire format.",
    repos: [
      "grafana/grafana",
      "prometheus/prometheus",
      "netdata/netdata",
      "signoz/signoz",
      "victoriametrics/victoriametrics",
      "open-telemetry/opentelemetry-collector",
    ],
  },
  {
    slug: "self-hosted-analytics",
    name: "Self-hosted analytics",
    short: "Plausible, Umami, PostHog, Matomo and Ackee compared.",
    intro:
      "Privacy regulation and cookie-banner fatigue turned self-hosted analytics from a hobbyist niche into a real market, and the star curves date it precisely: Plausible and Umami both inflect hard around 2020, right as the industry started looking for Google Analytics exits. PostHog grew a full product-analytics suite on the same wave, while Matomo — the elder of the category — shows what fifteen years of steady open-source persistence looks like.",
    repos: [
      "plausible/analytics",
      "umami-software/umami",
      "posthog/posthog",
      "matomo-org/matomo",
      "electerious/ackee",
    ],
  },
  {
    slug: "game-engines",
    name: "Open-source game engines",
    short: "Godot, Bevy, libGDX, Defold and O3DE star history.",
    intro:
      "Godot is the open-source success story of game development — years of patient growth, then a series of sharp accelerations every time a commercial engine gave developers a reason to look around. Bevy shows Rust's pull extending into games with an unusually steep curve for an engine still pre-1.0, while libGDX, Defold and O3DE each hold distinct niches. Engine choice is a decade-long commitment; growth trajectories are fair evidence.",
    repos: [
      "godotengine/godot",
      "bevyengine/bevy",
      "libgdx/libgdx",
      "defold/defold",
      "o3de/o3de",
    ],
  },
];

export type CategoryGroup = {
  name: string;
  description: string;
  categories: Category[];
};

export const CATEGORY_GROUPS: CategoryGroup[] = [
  {
    name: "Web development",
    description: "Frameworks, runtimes, styling, build tools and data access.",
    categories: CATEGORIES.slice(0, 7),
  },
  {
    name: "Developer ecosystems",
    description: "Language stacks, editors, terminals, AI tools and game engines.",
    categories: [...CATEGORIES.slice(7, 14), CATEGORIES[20]],
  },
  {
    name: "Data & infrastructure",
    description: "Databases, messaging, containers, observability and analytics.",
    categories: CATEGORIES.slice(14, 20),
  },
];

/** Look up a category by slug (exact, lowercase). */
export function getCategory(slug: string): Category | undefined {
  return CATEGORIES.find((c) => c.slug === slug);
}

/** All categories containing the given repo slug (lowercase owner/repo). */
export function categoriesForRepo(repoSlug: string): Category[] {
  const slug = repoSlug.toLowerCase();
  return CATEGORIES.filter((c) => c.repos.includes(slug));
}

/**
 * Deterministic related categories: the nearest neighbors in the curated
 * array (which is ordered so adjacency ≈ topical closeness). Alternates
 * next/prev so links spread both directions; wraps around the ends.
 */
export function relatedCategories(cat: Category, count = 4): Category[] {
  const i = CATEGORIES.findIndex((c) => c.slug === cat.slug);
  if (i === -1) return [];
  const out: Category[] = [];
  const n = CATEGORIES.length;
  for (let step = 1; out.length < Math.min(count, n - 1); step++) {
    const next = CATEGORIES[(i + step) % n];
    if (next.slug !== cat.slug && !out.includes(next)) out.push(next);
    if (out.length >= Math.min(count, n - 1)) break;
    const prev = CATEGORIES[(i - step + n * step) % n];
    if (prev.slug !== cat.slug && !out.includes(prev)) out.push(prev);
  }
  return out;
}

/**
 * Related repos for a repo page: members of the categories this repo
 * belongs to, self excluded, order-preserving and deduped. Empty when the
 * repo isn't in any curated category (callers fall back to a generic
 * popular set).
 */
export function relatedRepos(repoSlug: string, count = 6): string[] {
  const slug = repoSlug.toLowerCase();
  const out: string[] = [];
  for (const cat of categoriesForRepo(slug)) {
    for (const member of cat.repos) {
      if (member !== slug && !out.includes(member)) out.push(member);
      if (out.length >= count) return out;
    }
  }
  return out;
}
