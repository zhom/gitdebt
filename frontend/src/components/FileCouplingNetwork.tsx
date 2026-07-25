"use client";

import { useEffect, useMemo, useRef, useState } from "react";

import {
  BAYER4,
  INK,
  SWATCH,
  hashSeed,
  type RGB,
} from "@/lib/dither";
import {
  layoutFileCouplings,
  type CouplingEdge,
  type CouplingLayout,
  type FileCoupling,
} from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

export type FileCouplingNetworkProps = {
  couplings: readonly FileCoupling[];
  seed: string;
  className?: string;
};

const FRAME_MS = 50;
const CYCLE_MS = 3_200;
const CLUSTER_COLORS: readonly RGB[] = [
  SWATCH.blue,
  SWATCH.pink,
  SWATCH.green,
  SWATCH.orange,
  SWATCH.purple,
  SWATCH.red,
];

const compact = (value: number) =>
  new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);

const rgba = (color: RGB, alpha: number) =>
  `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${alpha})`;

function edgeLabel(edge: CouplingEdge): string {
  const source = edge.source.split("/").at(-1) || edge.source;
  const target = edge.target.split("/").at(-1) || edge.target;
  return `${source} ↔ ${target}`;
}

function clusterColor(layout: CouplingLayout, cluster: string): RGB {
  const index = Math.max(
    0,
    layout.clusters.findIndex((item) => item.id === cluster),
  );
  return CLUSTER_COLORS[index % CLUSTER_COLORS.length];
}

export function FileCouplingNetwork({
  couplings,
  seed,
  className,
}: FileCouplingNetworkProps) {
  const layout = useMemo(() => layoutFileCouplings(couplings), [couplings]);
  const [autoIndex, setAutoIndex] = useState(0);
  const [manualIndex, setManualIndex] = useState<number | null>(null);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const autoRef = useRef(autoIndex);
  const manualRef = useRef(manualIndex);
  const hoveredRef = useRef(hoveredNode);
  autoRef.current = autoIndex;
  manualRef.current = manualIndex;
  hoveredRef.current = hoveredNode;
  const activeIndex =
    layout.edges.length > 0
      ? (manualIndex ?? autoIndex) % layout.edges.length
      : 0;
  const active = layout.edges[activeIndex] ?? null;

  useEffect(() => {
    if (autoIndex >= layout.edges.length) setAutoIndex(0);
    if (manualIndex !== null && manualIndex >= layout.edges.length) {
      setManualIndex(null);
    }
  }, [autoIndex, layout.edges.length, manualIndex]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || layout.edges.length === 0) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    let width = 1;
    let height = 1;
    let frame = 0;
    let visible = true;
    let reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let lastPaint = 0;
    let lastCycle = -1;
    const started = performance.now();
    const seedPhase = (hashSeed(seed) & 2047) / 2047;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");

    const resizeCanvas = () => {
      const box = canvas.getBoundingClientRect();
      const nextWidth = Math.max(1, Math.round(box.width));
      const nextHeight = Math.max(1, Math.round(box.height));
      if (nextWidth === width && nextHeight === height) return;
      width = nextWidth;
      height = nextHeight;
      const dpr = Math.min(1.5, window.devicePixelRatio || 1);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const position = (id: string) => {
      const node = layout.nodes.find((item) => item.id === id);
      if (!node) return null;
      const insetX = Math.min(48, width * 0.1);
      const insetY = 30;
      return {
        node,
        x: insetX + node.x * Math.max(1, width - insetX * 2),
        y: insetY + node.y * Math.max(1, height - insetY * 2),
      };
    };

    const paintBackground = (phase: number) => {
      context.clearRect(0, 0, width, height);
      context.fillStyle = "rgba(237, 237, 237, 0.035)";
      const shift = reduced
        ? Math.floor(seedPhase * 4)
        : Math.floor(phase * 2 + seedPhase * 4) & 3;
      for (let y = 3; y < height; y += 6) {
        for (let x = 3; x < width; x += 6) {
          if (BAYER4[(Math.floor(y / 6) + shift) & 3][Math.floor(x / 6) & 3] < 0.3) {
            context.fillRect(x, y, 1, 1);
          }
        }
      }
    };

    const paintEdge = (
      edge: CouplingEdge,
      index: number,
      phase: number,
      activeEdge: number,
    ) => {
      const source = position(edge.source);
      const target = position(edge.target);
      if (!source || !target) return;
      const isActive =
        index === activeEdge ||
        hoveredRef.current === edge.source ||
        hoveredRef.current === edge.target;
      const color = isActive
        ? clusterColor(layout, edge.cluster)
        : INK;
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(1, Math.hypot(dx, dy));
      const steps = Math.max(4, Math.floor(distance / 4));
      const density = Math.min(0.94, 0.2 + edge.strength * 0.62);
      context.fillStyle = rgba(color, isActive ? 0.88 : 0.18 + edge.strength * 0.3);
      for (let step = 0; step <= steps; step += 1) {
        if (density <= BAYER4[(step + index) & 3][(step >> 2) & 3]) continue;
        const t = step / steps;
        context.fillRect(
          Math.round(source.x + dx * t) - 1,
          Math.round(source.y + dy * t) - 1,
          isActive ? 3 : 2,
          isActive ? 3 : 2,
        );
      }
      if (!isActive || reduced) return;
      context.fillStyle = rgba(color, 0.98);
      for (let packet = 0; packet < 3; packet += 1) {
        const t = (phase * 0.16 + seedPhase + packet / 3) % 1;
        context.fillRect(
          Math.round(source.x + dx * t) - 2,
          Math.round(source.y + dy * t) - 2,
          5,
          5,
        );
      }
    };

    const paintNode = (
      id: string,
      phase: number,
      activeEdge: CouplingEdge | null,
    ) => {
      const point = position(id);
      if (!point) return;
      const connected =
        activeEdge?.source === id ||
        activeEdge?.target === id ||
        hoveredRef.current === id;
      const maxWeight = Math.max(1, ...layout.nodes.map((node) => node.weight));
      const radius = 5 + Math.sqrt(point.node.weight / maxWeight) * 7;
      const color = clusterColor(layout, point.node.cluster);
      const shimmer = reduced ? 0 : Math.floor(phase * 2) & 3;
      for (let y = -radius; y <= radius; y += 2) {
        for (let x = -radius; x <= radius; x += 2) {
          if (x * x + y * y > radius * radius) continue;
          const density = connected ? 0.9 : 0.42;
          if (
            density <=
            BAYER4[(Math.round(y / 2) + shimmer) & 3][Math.round(x / 2) & 3]
          ) {
            continue;
          }
          context.fillStyle = rgba(color, connected ? 0.96 : 0.54);
          context.fillRect(
            Math.round(point.x + x),
            Math.round(point.y + y),
            connected ? 3 : 2,
            connected ? 3 : 2,
          );
        }
      }
      if (!connected) return;
      context.font =
        '11px "Geist Mono Variable", ui-monospace, monospace';
      context.textBaseline = "middle";
      const metrics = context.measureText(point.node.label);
      const labelX = Math.max(
        metrics.width / 2 + 4,
        Math.min(width - metrics.width / 2 - 4, point.x),
      );
      const labelY =
        point.y < height * 0.25 ? point.y + radius + 13 : point.y - radius - 10;
      context.fillStyle = "rgba(8, 9, 12, 0.86)";
      context.fillRect(
        labelX - metrics.width / 2 - 4,
        labelY - 8,
        metrics.width + 8,
        16,
      );
      context.textAlign = "center";
      context.fillStyle = "rgba(237, 237, 237, 0.92)";
      context.fillText(point.node.label, labelX, labelY);
    };

    const paint = (now: number) => {
      resizeCanvas();
      const phase = reduced ? 0 : (now - started) / 1_000;
      if (!reduced && manualRef.current === null) {
        const cycle = Math.floor((now - started) / CYCLE_MS);
        if (cycle !== lastCycle) {
          lastCycle = cycle;
          const next = cycle % layout.edges.length;
          if (next !== autoRef.current) {
            autoRef.current = next;
            setAutoIndex(next);
          }
        }
      }
      const edgeIndex =
        (manualRef.current ?? autoRef.current) % layout.edges.length;
      const activeEdge = layout.edges[edgeIndex] ?? null;
      paintBackground(phase);
      layout.edges.forEach((edge, index) =>
        paintEdge(edge, index, phase, edgeIndex),
      );
      layout.nodes.forEach((node) => paintNode(node.id, phase, activeEdge));
    };

    const tick = (now: number) => {
      frame = 0;
      if (!visible) return;
      if (now - lastPaint >= FRAME_MS) {
        lastPaint = now;
        paint(now);
      }
      if (!reduced) frame = requestAnimationFrame(tick);
    };
    const start = () => {
      if (reduced) paint(performance.now());
      else if (!frame && visible) frame = requestAnimationFrame(tick);
    };
    const stop = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
    };
    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true;
      if (visible) start();
      else stop();
    });
    intersection.observe(canvas);
    const resize = new ResizeObserver(() => paint(performance.now()));
    resize.observe(canvas);
    const updateMotion = () => {
      reduced = motion.matches;
      stop();
      start();
    };
    motion.addEventListener("change", updateMotion);
    start();

    return () => {
      stop();
      intersection.disconnect();
      resize.disconnect();
      motion.removeEventListener("change", updateMotion);
    };
  }, [layout, seed]);

  const nearestNode = (clientX: number, clientY: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const box = canvas.getBoundingClientRect();
    const insetX = Math.min(48, box.width * 0.1);
    const insetY = 30;
    let nearest: { id: string; distance: number } | null = null;
    for (const node of layout.nodes) {
      const x = insetX + node.x * Math.max(1, box.width - insetX * 2);
      const y = insetY + node.y * Math.max(1, box.height - insetY * 2);
      const distance = Math.hypot(clientX - box.left - x, clientY - box.top - y);
      if (!nearest || distance < nearest.distance) {
        nearest = { id: node.id, distance };
      }
    }
    return nearest && nearest.distance <= 22 ? nearest.id : null;
  };

  if (layout.edges.length === 0) {
    return (
      <p
        className={cn(
          "p-3.5 text-pretty text-base text-muted-foreground sm:text-sm",
          className,
        )}
      >
        No repeated file relationships were strong enough to visualize.
      </p>
    );
  }

  return (
    <figure className={cn("min-w-0 p-3.5", className)}>
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="h-72 w-full max-w-full touch-pan-y overflow-hidden rounded-md bg-background/35"
        onPointerMove={(event) =>
          setHoveredNode(nearestNode(event.clientX, event.clientY))
        }
        onPointerLeave={() => setHoveredNode(null)}
      />

      <figcaption className="mt-3 grid gap-2 sm:grid-cols-2">
        {layout.edges.slice(0, 4).map((edge, index) => (
          <button
            key={`${edge.source}\0${edge.target}`}
            type="button"
            aria-label={`${edge.source} and ${edge.target}: ${edge.cochanges} co-changes and ${edge.fixCommits} fix commits`}
            aria-pressed={index === activeIndex}
            title={`${edge.source} ↔ ${edge.target}`}
            onClick={() => setManualIndex(index)}
            onPointerEnter={() => setManualIndex(index)}
            onPointerLeave={() => setManualIndex(null)}
            onFocus={() => setManualIndex(index)}
            onBlur={() => setManualIndex(null)}
            className="min-w-0 rounded-md border border-border/60 p-2.5 text-left outline-none transition-transform duration-200 hover:-translate-y-0.5 focus-visible:ring-2 focus-visible:ring-accent/30 aria-pressed:border-foreground/30 aria-pressed:bg-card/70 motion-reduce:transition-none"
          >
            <p className="truncate font-mono text-base text-foreground/90 sm:text-[0.6875rem]">
              {edgeLabel(edge)}
            </p>
            <div className="mt-1 flex items-center justify-between gap-2 font-mono text-sm text-muted-foreground tabular-nums sm:text-[0.625rem]">
              <p>{compact(edge.cochanges)} co-changes</p>
              <p>{compact(edge.fixCommits)} fixes</p>
            </div>
          </button>
        ))}
      </figcaption>
      <ul role="list" className="sr-only">
        {layout.edges.map((edge) => (
          <li key={`${edge.source}\0${edge.target}-accessible`}>
            {edge.source} and {edge.target}: {edge.cochanges} co-changes,{" "}
            {edge.fixCommits} fix commits.
          </li>
        ))}
      </ul>
      <p
        className="mt-3 text-pretty text-base text-muted-foreground sm:text-sm"
        aria-live="polite"
      >
        {active
          ? `${active.source} and ${active.target} changed together ${active.cochanges.toLocaleString()} times; ${active.fixCommits.toLocaleString()} of the coupled changes were fix commits.`
          : "Strongest file relationships are unavailable."}
      </p>
    </figure>
  );
}
