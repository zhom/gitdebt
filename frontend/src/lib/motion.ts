export const EASE_OUT = [0.23, 1, 0.32, 1] as const;
export const EASE_IN_OUT = [0.77, 0, 0.175, 1] as const;

export const DURATION = {
  press: 0.12,
  feedback: 0.14,
  enter: 0.18,
  move: 0.2,
  chart: 0.22,
} as const;

export const REDUCED_MOTION_DURATION = 0.12;

export const SPRING = {
  snappy: { type: "spring", stiffness: 420, damping: 34, mass: 0.72 },
} as const;
