import { useEffect, useRef } from "react";

import { asciiDensity, asciiGlyph, asciiGrid } from "@/lib/ascii-dither";
import { BAYER4 } from "@/lib/dither";
import { isBrowser, metricsFor, naturalWidth } from "@/lib/pretext";

type Props = {
  path: string;
};

const SOURCE = {
  x: 41.436,
  y: 108.392,
  width: 429.115,
  height: 299.305,
};
const SAMPLE = 3;
const FRAME_MS = 50;

function logoMask(
  path: string,
  width: number,
  height: number,
  cols: number,
  rows: number,
): Float32Array | null {
  const mask = document.createElement("canvas");
  mask.width = cols * SAMPLE;
  mask.height = rows * SAMPLE;
  const context = mask.getContext("2d", { willReadFrequently: true });
  if (!context) return null;

  let logo: Path2D;
  try {
    logo = new Path2D(path);
  } catch {
    return null;
  }

  const sourceRatio = SOURCE.width / SOURCE.height;
  const wideFooter = width / Math.max(1, height) > 1.45;
  const artWidth = wideFooter ? height * 1.38 * sourceRatio : width * 1.55;
  const artHeight = artWidth / sourceRatio;
  const artX = (width - artWidth) / 2;
  const artY = height - artHeight * (wideFooter ? 0.76 : 0.88);
  const scale = artWidth / SOURCE.width;

  context.save();
  context.scale(mask.width / width, mask.height / height);
  context.translate(artX, artY);
  context.scale(scale, scale);
  context.translate(-SOURCE.x, -SOURCE.y);
  context.fillStyle = "#fff";
  context.fill(logo);
  context.restore();

  const pixels = context.getImageData(0, 0, mask.width, mask.height).data;
  const samples = new Float32Array(cols * rows);
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      let alpha = 0;
      for (let sy = 0; sy < SAMPLE; sy++) {
        for (let sx = 0; sx < SAMPLE; sx++) {
          const px = col * SAMPLE + sx;
          const py = row * SAMPLE + sy;
          alpha += pixels[(py * mask.width + px) * 4 + 3] / 255;
        }
      }
      samples[row * cols + col] = alpha / (SAMPLE * SAMPLE);
    }
  }
  return samples;
}

/**
 * The footer's oversized robot, rebuilt as an animated hexdump mask.
 *
 * Pretext measures the real loaded mono font, so glyph cells fit without a
 * guessed `ch` width. The logo path is sampled into those cells, then the
 * shared Bayer matrix decides which hex characters survive the dither.
 */
export function FooterAsciiMark({ path }: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const probeRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const root = rootRef.current;
    const probe = probeRef.current;
    const canvas = canvasRef.current;
    if (!root || !probe || !canvas || !isBrowser()) return;

    const context = canvas.getContext("2d");
    if (!context) return;

    let frame = 0;
    let lastFrame = 0;
    let visible = true;
    let reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let drawFrame: ((time: number) => void) | null = null;

    const draw = () => {
      const width = root.clientWidth;
      const height = root.clientHeight;
      if (width <= 0 || height <= 0) return;

      const { font, lineHeight, letterSpacing } = metricsFor(probe);
      const glyphWidth = naturalWidth("0", font, letterSpacing);
      const grid = asciiGrid(width, height, glyphWidth, lineHeight);
      const mask = logoMask(path, width, height, grid.cols, grid.rows);
      if (!mask) return;

      const dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.font = font;
      context.fillStyle = getComputedStyle(root).color;

      drawFrame = (time: number) => {
        if (!visible) return;
        if (!reduced && time - lastFrame < FRAME_MS) {
          frame = requestAnimationFrame(drawFrame!);
          return;
        }
        lastFrame = time;
        const phase = reduced ? 0 : time / 1_900;
        context.clearRect(0, 0, width, height);
        for (let row = 0; row < grid.rows; row++) {
          for (let col = 0; col < grid.cols; col++) {
            const alpha = mask[row * grid.cols + col];
            if (alpha < 0.035) continue;
            const density = asciiDensity(
              alpha,
              col,
              row,
              grid.cols,
              grid.rows,
              phase,
            );
            if (density <= BAYER4[row & 3][col & 3]) continue;
            context.globalAlpha = 0.34 + density * 0.6;
            context.fillText(
              asciiGlyph(col, row, phase),
              (col + 0.5) * grid.cellWidth,
              (row + 0.5) * grid.cellHeight,
            );
          }
        }
        context.globalAlpha = 1;
        if (!reduced) frame = requestAnimationFrame(drawFrame!);
      };

      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(drawFrame);
    };

    const resize = new ResizeObserver(draw);
    resize.observe(root);
    const visibility = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true;
      if (visible && drawFrame && !reduced) {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(drawFrame);
      }
    });
    visibility.observe(root);
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotion = () => {
      reduced = motion.matches;
      draw();
    };
    motion.addEventListener("change", updateMotion);
    draw();

    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      visibility.disconnect();
      motion.removeEventListener("change", updateMotion);
    };
  }, [path]);

  return (
    <div
      ref={rootRef}
      className="pointer-events-none absolute inset-0 -z-10 overflow-hidden text-foreground opacity-[0.16] select-none"
      aria-hidden="true"
    >
      <div
        ref={probeRef}
        className="pointer-events-none absolute font-mono text-[10px] leading-3 opacity-0"
      >
        0
      </div>
      <canvas
        ref={canvasRef}
        className="size-full [image-rendering:pixelated]"
      />
    </div>
  );
}
