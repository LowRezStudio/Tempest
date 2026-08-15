import { computeDropIndex, computeGridDropIndex, measureGrid, reorderArray } from "./reorder";
import type { SlotRect } from "./reorder";

export type ReorderDragState<T> = {
	readonly id: string;
	readonly from: number;
	readonly item: T;
	readonly offsetX: number;
	readonly offsetY: number;
	readonly width: number;
	readonly orderedIds: readonly string[];
	readonly rects: ReadonlyMap<string, SlotRect>;
	readonly columns: number;
	readonly xPos: number[];
	readonly yPos: number[];
};

export type ReorderableOptions = {
	readonly ids: () => readonly string[];
	readonly container: () => HTMLElement | undefined;
	readonly onReorder: (ids: string[]) => void;
	/** Enable row-major (multi-column grid) drop semantics instead of a single column. */
	readonly grid?: boolean;
};

const DRAG_THRESHOLD = 4;
const DRAG_BODY_CLASS = "sidebar-dragging";

type Pending<T> = {
	readonly id: string;
	readonly index: number;
	readonly item: T;
	readonly startX: number;
	readonly startY: number;
};

export function createReorderable<T>(options: ReorderableOptions) {
	let drag = $state<ReorderDragState<T> | null>(null);
	let toIndex = $state(0);
	let pointerX = $state(0);
	let pointerY = $state(0);
	let pending: Pending<T> | null = null;

	function onPointerMove(e: PointerEvent) {
		if (!pending) return;
		if (!drag) {
			if (
				Math.abs(e.clientY - pending.startY) < DRAG_THRESHOLD &&
				Math.abs(e.clientX - pending.startX) < DRAG_THRESHOLD
			) {
				return;
			}
			startDrag(e);
		}
		if (drag) {
			pointerX = e.clientX;
			pointerY = e.clientY;
			toIndex = options.grid
				? computeGridDropIndex(drag.orderedIds, drag.rects, e.clientX, e.clientY)
				: computeDropIndex(drag.orderedIds, drag.rects, e.clientY);
		}
	}

	function startDrag(e: PointerEvent) {
		if (!pending) return;
		const container = options.container();
		if (!container) return;
		const slots = container.querySelectorAll<HTMLElement>("[data-id]");
		const orderedIds: string[] = [];
		const rects = new Map<string, SlotRect>();
		for (const slot of slots) {
			const id = slot.dataset.id;
			if (!id) continue;
			const rect = slot.getBoundingClientRect();
			orderedIds.push(id);
			rects.set(id, {
				top: rect.top,
				bottom: rect.bottom,
				left: rect.left,
				mid: rect.top + rect.height / 2,
				cx: rect.left + rect.width / 2,
			});
		}
		const { columns, xPos, yPos } = measureGrid(orderedIds, rects);
		const draggedSlot = container.querySelector<HTMLElement>(
			`[data-id="${CSS.escape(pending.id)}"]`,
		);
		const rect = draggedSlot?.getBoundingClientRect();
		if (!rect) return;
		drag = {
			id: pending.id,
			from: pending.index,
			item: pending.item,
			offsetX: e.clientX - rect.left,
			offsetY: e.clientY - rect.top,
			width: rect.width,
			orderedIds,
			rects,
			columns,
			xPos,
			yPos,
		};
		toIndex = pending.index;
		pointerX = e.clientX;
		pointerY = e.clientY;
	}

	function onPointerUp() {
		window.removeEventListener("pointermove", onPointerMove);
		if (drag) {
			const ids = options.ids();
			const from = ids.indexOf(drag.id);
			if (from >= 0 && toIndex !== from) {
				options.onReorder(reorderArray(ids, from, toIndex));
			}
			// Suppress the click that follows a drag so we don't navigate.
			window.addEventListener(
				"click",
				(ev) => {
					ev.preventDefault();
					ev.stopPropagation();
				},
				{ capture: true, once: true },
			);
			// Drop focus so the tooltip (shown on :focus-within) doesn't stick.
			if (document.activeElement instanceof HTMLElement) {
				document.activeElement.blur();
			}
		}
		pending = null;
		drag = null;
	}

	function pointerdown(id: string, index: number, item: T) {
		return (e: PointerEvent) => {
			if (e.button !== 0) return;
			pending = { id, index, item, startX: e.clientX, startY: e.clientY };
			window.addEventListener("pointermove", onPointerMove);
			window.addEventListener("pointerup", onPointerUp, { once: true });
		};
	}

	function shiftFor(index: number): string {
		if (!drag) return "";
		const from = drag.from;
		const to = toIndex;
		if (to === from) return "";
		let moved = index;
		if (to < from && index >= to && index < from) moved = index + 1;
		else if (to > from && index > from && index <= to) moved = index - 1;
		else return "";
		const c0 = index % drag.columns;
		const r0 = Math.floor(index / drag.columns);
		const c1 = moved % drag.columns;
		const r1 = Math.floor(moved / drag.columns);
		const dx = (drag.xPos[c1] ?? 0) - (drag.xPos[c0] ?? 0);
		const dy = (drag.yPos[r1] ?? 0) - (drag.yPos[r0] ?? 0);
		if (dx === 0 && dy === 0) return "";
		return `translate(${dx}px, ${dy}px)`;
	}

	$effect(() => {
		const active = drag !== null;
		document.body.classList.toggle(DRAG_BODY_CLASS, active);
		return () => document.body.classList.remove(DRAG_BODY_CLASS);
	});

	return {
		get drag() {
			return drag;
		},
		get toIndex() {
			return toIndex;
		},
		get pointerX() {
			return pointerX;
		},
		get pointerY() {
			return pointerY;
		},
		pointerdown,
		shiftFor,
	};
}
