/**
 * Options for Svelte's `Spring`: critically damped, so it never overshoots, settling in about
 * 0.4 s. Its `dt` counts 60 Hz frames, so stiffness is ω² and damping 2ω for a 0.4 s period.
 */
export const SMOOTH_SPRING = { stiffness: 0.07, damping: 0.53 };
