"use client";

import { useEffect, useRef } from "react";

import {
  ambientOrbitPoint,
  ambientWave,
  brailleGlyph,
  normalizeSignals,
  signalHash,
  signalSeed,
} from "@/lib/signal-art";
import { cn } from "@/lib/utils";

type Props = {
  mode: "repository" | "profile";
  seed: string;
  values: readonly number[];
  className?: string;
};

const FRAME_MS = 55;
const BLUE = [53, 143, 243] as const;
const PINK = [240, 90, 190] as const;
const REPO_GLYPHS = "01<>/\\[]{}+=*#~";

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

function clearReadingField(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  centerX = 0.5,
  reach = 0.58,
) {
  context.save();
  context.globalCompositeOperation = "destination-out";
  const focus = context.createRadialGradient(
    width * centerX,
    height * 0.45,
    Math.min(width, height) * 0.08,
    width * centerX,
    height * 0.45,
    Math.max(width, height) * reach,
  );
  focus.addColorStop(0, "rgba(0,0,0,0.94)");
  focus.addColorStop(0.48, "rgba(0,0,0,0.72)");
  focus.addColorStop(1, "rgba(0,0,0,0)");
  context.fillStyle = focus;
  context.fillRect(0, 0, width, height);
  context.restore();
}

function clearProfileReadingField(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
) {
  context.save();
  context.globalCompositeOperation = "destination-out";
  const focus = context.createLinearGradient(0, 0, width * 0.78, 0);
  focus.addColorStop(0, "rgba(0,0,0,0.92)");
  focus.addColorStop(0.58, "rgba(0,0,0,0.7)");
  focus.addColorStop(1, "rgba(0,0,0,0)");
  context.fillStyle = focus;
  context.fillRect(0, 0, width, height);
  context.restore();
}

/** Slow ASCII currents shaped by the repository's star-history values. */
function paintRepositoryCurrent(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  seed: number,
  phase: number,
  values: readonly number[],
) {
  const signals = normalizeSignals(values);
  const bands = width < 640 ? 3 : 4;
  const step = width < 640 ? 22 : 18;
  const columns = Math.ceil(width / step) + 4;

  context.font = '11px "Geist Mono Variable", ui-monospace, monospace';
  context.textAlign = "center";
  context.textBaseline = "middle";

  for (let band = 0; band < bands; band += 1) {
    const strength = signals[band % signals.length] ?? 0.5;
    const color = mixRgb(BLUE, PINK, band / Math.max(1, bands - 1));
    const drift = phase * (5 + band * 1.6);

    context.beginPath();
    for (let column = -2; column < columns; column += 1) {
      const x = ((column * step + drift) % (width + step * 2)) - step;
      const u = x / Math.max(1, width);
      const y = ambientWave(seed, u, band, phase, values) * height;
      if (column === -2) context.moveTo(x, y);
      else context.lineTo(x, y);

      const sample = signalHash(
        seed ^ (band * 0x9e37),
        column + Math.floor(drift / step),
        band,
      );
      const glyph = REPO_GLYPHS[Math.floor(sample * REPO_GLYPHS.length)] ?? "0";
      const energy = 0.2 + strength * 0.34 + sample * 0.22;
      context.fillStyle = rgba(color, energy);
      context.shadowColor = rgba(color, energy * 0.36);
      context.shadowBlur = sample > 0.76 ? 7 : 0;
      context.fillText(glyph, x, y);

      // Ordered fragments trail each current so the line reads as dither, not
      // as decorative wallpaper.
      for (let trail = 1; trail <= 4; trail += 1) {
        const threshold = signalHash(seed, column, band * 11 + trail);
        if (threshold < trail * 0.14) continue;
        const direction = band % 2 === 0 ? 1 : -1;
        context.fillStyle = rgba(color, energy * (0.3 / trail));
        context.fillRect(
          Math.round(x / 4) * 4,
          Math.round((y + direction * trail * 8) / 4) * 4,
          trail === 1 ? 3 : 2,
          trail === 1 ? 3 : 2,
        );
      }
    }
    context.shadowBlur = 0;
    context.strokeStyle = rgba(color, 0.08 + strength * 0.06);
    context.lineWidth = 1;
    context.stroke();
  }

  clearReadingField(context, width, height);
}

/** Orbiting braille packets turn aggregate profile activity into a halo. */
function paintProfileOrbit(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  seed: number,
  phase: number,
  values: readonly number[],
) {
  const rings = width < 640 ? 3 : 5;

  context.font = '12px "Geist Mono Variable", ui-monospace, monospace';
  context.textAlign = "center";
  context.textBaseline = "middle";

  for (let ring = 0; ring < rings; ring += 1) {
    const points = 34 + ring * 9;
    const color = mixRgb(BLUE, PINK, ring / Math.max(1, rings - 1));
    const packet = Math.floor(
      (phase * (2.1 + ring * 0.42) + ring * 7) % points,
    );

    for (let index = 0; index < points; index += 1) {
      const point = ambientOrbitPoint(
        seed,
        index,
        points,
        ring,
        phase,
        values,
      );
      const x = point.x * width;
      const y = point.y * height;
      const packetDistance = Math.min(
        (index - packet + points) % points,
        (packet - index + points) % points,
      );
      const pulse = Math.max(0, 1 - packetDistance / 6);
      const alpha = 0.2 + point.energy * 0.34 + pulse * 0.58;

      context.fillStyle = rgba(color, alpha);
      context.shadowColor = rgba(color, pulse * 0.32);
      context.shadowBlur = pulse > 0.35 ? 7 : 0;
      context.fillText(
        brailleGlyph(seed ^ ring, index, ring, phase, values),
        x,
        y,
      );

      if (pulse > 0.2) {
        const size = pulse > 0.72 ? 3 : 2;
        context.fillStyle = rgba(color, pulse * 0.62);
        context.fillRect(
          Math.round((x + 8) / 4) * 4,
          Math.round((y - 5) / 4) * 4,
          size,
          size,
        );
      }
    }
  }
  context.shadowBlur = 0;

  // Sparse checksum labels give the halo an ASCII identity without turning it
  // into a wall of illegible text.
  context.font = '9px "Geist Mono Variable", ui-monospace, monospace';
  for (let index = 0; index < 12; index += 1) {
    const x = signalHash(seed, index, 71) * width;
    const y = signalHash(seed, index, 113) * height;
    const byte = Math.floor(signalHash(seed, index, 149) * 255)
      .toString(16)
      .padStart(2, "0")
      .toUpperCase();
    context.fillStyle = rgba(index % 2 === 0 ? BLUE : PINK, 0.22);
    context.fillText(`0x${byte}`, x, y);
  }

  clearProfileReadingField(context, width, height);
}

export function PageAmbientSignal({
  mode,
  seed,
  values,
  className,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const valueKey = values.join(",");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d", { alpha: true });
    if (!context) return;

    const numericSeed = signalSeed(seed);
    const signalValues = valueKey.split(",").map(Number);
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let reduced = motion.matches;
    let frame = 0;
    let lastFrame = 0;
    let width = 0;
    let height = 0;
    const started = performance.now();

    const resizeCanvas = () => {
      const nextWidth = Math.max(1, window.innerWidth);
      const nextHeight = Math.max(1, window.innerHeight);
      if (nextWidth === width && nextHeight === height) return;
      width = nextWidth;
      height = nextHeight;
      const dpr = Math.min(1.25, window.devicePixelRatio || 1);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    const paint = (now: number) => {
      resizeCanvas();
      context.clearRect(0, 0, width, height);
      const phase = reduced ? 2.4 : (now - started) / 1_000;
      if (mode === "repository") {
        paintRepositoryCurrent(
          context,
          width,
          height,
          numericSeed,
          phase,
          signalValues,
        );
      } else {
        paintProfileOrbit(
          context,
          width,
          height,
          numericSeed,
          phase,
          signalValues,
        );
      }
    };

    const tick = (now: number) => {
      frame = 0;
      if (document.visibilityState !== "visible") return;
      if (now - lastFrame >= FRAME_MS) {
        lastFrame = now;
        paint(now);
      }
      if (!reduced) frame = requestAnimationFrame(tick);
    };
    const start = () => {
      if (reduced) paint(performance.now());
      else if (!frame && document.visibilityState === "visible") {
        frame = requestAnimationFrame(tick);
      }
    };
    const stop = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
    };
    const updateMotion = () => {
      reduced = motion.matches;
      stop();
      start();
    };
    const updateVisibility = () => {
      stop();
      start();
    };

    window.addEventListener("resize", resizeCanvas, { passive: true });
    document.addEventListener("visibilitychange", updateVisibility);
    motion.addEventListener("change", updateMotion);
    start();

    return () => {
      stop();
      window.removeEventListener("resize", resizeCanvas);
      document.removeEventListener("visibilitychange", updateVisibility);
      motion.removeEventListener("change", updateMotion);
    };
  }, [mode, seed, valueKey]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className={cn(
        "pointer-events-none fixed inset-0 -z-10 size-full [image-rendering:pixelated]",
        mode === "profile" ? "opacity-70" : "opacity-50",
        className,
      )}
    />
  );
}
