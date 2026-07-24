"use client";

import { useEffect, useRef } from "react";

import {
  brailleGlyph,
  contourSignal,
  mosaicSignal,
  normalizeSignals,
  signalHash,
  signalSeed,
  type SignalArtMode,
} from "@/lib/signal-art";
import { cn } from "@/lib/utils";

type Props = {
  mode: SignalArtMode;
  seed: string;
  values: readonly number[];
  className?: string;
};

const FRAME_MS = 50;
const BLUE = [53, 143, 243] as const;
const PINK = [255, 60, 172] as const;

function mixRgb(a: readonly number[], b: readonly number[], t: number) {
  return [
    Math.round(a[0] + (b[0] - a[0]) * t),
    Math.round(a[1] + (b[1] - a[1]) * t),
    Math.round(a[2] + (b[2] - a[2]) * t),
  ] as const;
}

function rgba(rgb: readonly number[], alpha: number) {
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${alpha})`;
}

function paintPostEffects(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
) {
  context.save();
  context.globalAlpha = 0.075;
  context.fillStyle = "#fff";
  for (let y = 1; y < height; y += 4) context.fillRect(0, y, width, 1);
  context.globalAlpha = 1;
  const vignette = context.createRadialGradient(
    width * 0.5,
    height * 0.48,
    Math.min(width, height) * 0.12,
    width * 0.5,
    height * 0.48,
    Math.max(width, height) * 0.66,
  );
  vignette.addColorStop(0, "rgba(0,0,0,0)");
  vignette.addColorStop(1, "rgba(0,0,0,0.62)");
  context.fillStyle = vignette;
  context.fillRect(0, 0, width, height);
  context.restore();
}

function paintMosaic(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  seed: number,
  phase: number,
  values: readonly number[],
) {
  const cell = width < 420 ? 12 : 16;
  const cols = Math.max(12, Math.ceil(width / cell));
  const rows = Math.max(6, Math.ceil(height / cell));
  context.fillStyle = "#08090c";
  context.fillRect(0, 0, width, height);

  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const signal = mosaicSignal(seed, col, row, cols, rows, phase, values);
      if (signal < 0.16) continue;
      const chroma = mixRgb(BLUE, PINK, Math.min(1, signal * 0.86));
      const x = col * cell;
      const y = row * cell;
      const inset = signal > 0.72 ? 1 : 2;

      if (signal > 0.66) {
        context.fillStyle = rgba(PINK, signal * 0.09);
        context.fillRect(x - 2, y, cell, cell);
        context.fillStyle = rgba(BLUE, signal * 0.1);
        context.fillRect(x + 2, y, cell, cell);
      }
      context.shadowColor = rgba(chroma, signal > 0.78 ? 0.32 : 0);
      context.shadowBlur = signal > 0.78 ? 8 : 0;
      context.fillStyle = rgba(chroma, 0.16 + signal * 0.68);
      context.fillRect(
        x + inset,
        y + inset,
        Math.max(1, cell - inset * 2),
        Math.max(1, cell - inset * 2),
      );
    }
  }
  context.shadowBlur = 0;
}

function paintBraille(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  seed: number,
  phase: number,
  values: readonly number[],
) {
  context.fillStyle = "#08090c";
  context.fillRect(0, 0, width, height);
  const cellWidth = 12;
  const cellHeight = 16;
  const cols = Math.ceil(width / cellWidth);
  const rows = Math.ceil(height / cellHeight);
  const signals = normalizeSignals(values);
  context.font = '13px "Geist Mono Variable", ui-monospace, monospace';
  context.textAlign = "center";
  context.textBaseline = "middle";

  for (let column = 0; column < cols; column += 1) {
    const strength = signals[column % signals.length] ?? 0.5;
    const head = (phase * (0.7 + strength) + signalHash(seed, column, 0) * rows) % rows;
    for (let row = 0; row < rows; row += 1) {
      const distance = (head - row + rows) % rows;
      const trail = Math.max(0, 1 - distance / Math.max(4, rows * 0.72));
      if (trail < 0.08 || signalHash(seed, column, row) > 0.88) continue;
      const color = mixRgb(BLUE, PINK, strength * 0.62 + trail * 0.24);
      context.shadowColor = rgba(color, trail * 0.3);
      context.shadowBlur = trail > 0.72 ? 6 : 0;
      context.fillStyle = rgba(color, 0.12 + trail * 0.72);
      context.fillText(
        brailleGlyph(seed, column, row, phase, values),
        column * cellWidth + cellWidth / 2,
        row * cellHeight + cellHeight / 2,
      );
    }
  }
  context.shadowBlur = 0;
}

function paintContour(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  seed: number,
  phase: number,
  values: readonly number[],
) {
  context.fillStyle = "#08090c";
  context.fillRect(0, 0, width, height);
  const gap = width < 420 ? 9 : 12;
  const cols = Math.ceil(width / gap);
  const rows = Math.ceil(height / gap);
  const levels = [0.28, 0.42, 0.56, 0.7];

  for (let levelIndex = 0; levelIndex < levels.length; levelIndex += 1) {
    const level = levels[levelIndex];
    const color = mixRgb(BLUE, PINK, levelIndex / (levels.length - 1));
    context.strokeStyle = rgba(color, 0.3 + levelIndex * 0.12);
    context.lineWidth = levelIndex === levels.length - 1 ? 1.5 : 1;
    context.shadowColor = rgba(color, 0.22);
    context.shadowBlur = levelIndex >= 2 ? 5 : 0;
    context.beginPath();
    for (let row = 0; row < rows - 1; row += 1) {
      for (let col = 0; col < cols - 1; col += 1) {
        const a = contourSignal(seed, col / cols, row / rows, phase, values);
        const b = contourSignal(seed, (col + 1) / cols, row / rows, phase, values);
        const c = contourSignal(
          seed,
          col / cols,
          (row + 1) / rows,
          phase,
          values,
        );
        const crossesX = (a < level) !== (b < level);
        const crossesY = (a < level) !== (c < level);
        if (!crossesX && !crossesY) continue;
        const x = col * gap + (crossesX ? gap * 0.5 : 0);
        const y = row * gap + (crossesY ? gap * 0.5 : 0);
        context.moveTo(x, y);
        context.lineTo(
          x + (crossesY ? gap * 0.72 : gap * 0.18),
          y + (crossesX ? gap * 0.72 : gap * 0.18),
        );
      }
    }
    context.stroke();
  }
  context.shadowBlur = 0;

  context.font = '9px "Geist Mono Variable", ui-monospace, monospace';
  context.textAlign = "center";
  context.textBaseline = "middle";
  for (let row = 1; row < rows; row += 3) {
    for (let col = 1; col < cols; col += 5) {
      if (signalHash(seed, col, row) < 0.56) continue;
      const byte = Math.floor(signalHash(seed ^ 0x9e37, col, row) * 255);
      context.fillStyle = rgba(col % 2 === 0 ? BLUE : PINK, 0.36);
      context.fillText(
        byte.toString(16).padStart(2, "0").toUpperCase(),
        col * gap,
        row * gap,
      );
    }
  }
}

export function PageSignalCanvas({ mode, seed, values, className }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const valueKey = values.join(",");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) return;

    const numericSeed = signalSeed(seed);
    const signalValues = valueKey.split(",").map(Number);
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let reduced = motion.matches;
    let visible = true;
    let frame = 0;
    let lastFrame = 0;
    let width = 0;
    let height = 0;
    const started = performance.now();

    const resizeCanvas = () => {
      const rect = canvas.getBoundingClientRect();
      const nextWidth = Math.max(1, Math.round(rect.width));
      const nextHeight = Math.max(1, Math.round(rect.height));
      if (nextWidth === width && nextHeight === height) return;
      width = nextWidth;
      height = nextHeight;
      const dpr = Math.min(1.5, window.devicePixelRatio || 1);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    const paint = (now: number) => {
      resizeCanvas();
      const phase = reduced ? 1.8 : (now - started) / 1_850;
      context.clearRect(0, 0, width, height);
      if (mode === "mosaic") {
        paintMosaic(context, width, height, numericSeed, phase, signalValues);
      } else if (mode === "braille") {
        paintBraille(context, width, height, numericSeed, phase, signalValues);
      } else {
        paintContour(context, width, height, numericSeed, phase, signalValues);
      }
      paintPostEffects(context, width, height);
    };

    const tick = (now: number) => {
      frame = 0;
      if (!visible) return;
      if (now - lastFrame >= FRAME_MS) {
        lastFrame = now;
        paint(now);
      }
      if (!reduced) frame = requestAnimationFrame(tick);
    };
    const start = () => {
      if (reduced) {
        paint(performance.now());
      } else if (!frame && visible) {
        frame = requestAnimationFrame(tick);
      }
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
  }, [mode, seed, valueKey]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className={cn(
        "size-full [image-rendering:pixelated]",
        className,
      )}
    />
  );
}
