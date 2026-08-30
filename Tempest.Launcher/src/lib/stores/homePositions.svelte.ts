import { persistedState } from "./persisted.svelte";

type Pos = { x: number; y: number } | null;

export const tilePosStore = persistedState<Pos>("home_tiles_position_v2", null);
export const btnPosStore = persistedState<Pos>("home_runbtn_position_v1", null);
export const hintPosStore = persistedState<Pos>("home_hinttiles_position_v1", null);

export function resetAllHomePositions() {
	tilePosStore.value = null;
	btnPosStore.value = null;
	hintPosStore.value = null;
}

export function clampPos(pos: Pos, el: HTMLElement, margin: number): Pos | null {
	if (pos === null) return null;
	const frame = el.offsetParent as HTMLElement | null;
	const frameW = frame?.clientWidth ?? window.innerWidth;
	const frameH = frame?.clientHeight ?? window.innerHeight;
	const maxX = Math.max(frameW - el.offsetWidth - margin, margin);
	const maxY = Math.max(frameH - el.offsetHeight - margin, margin);
	return {
		x: Math.min(Math.max(pos.x, margin), maxX),
		y: Math.min(Math.max(pos.y, margin), maxY),
	};
}
