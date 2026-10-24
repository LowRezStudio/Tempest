import type { SidebarTab, SidebarZone } from "$lib/stores/sidebar.svelte";

export type DropTarget = { zone: SidebarZone; index: number };

type Slot = DropTarget & { y: number };

export type TabDragState = {
	readonly id: SidebarTab;
	readonly from: DropTarget;
	readonly offsetX: number;
	readonly offsetY: number;
	readonly height: number;
	/** Every place the tab can land, by the centre it would have there */
	readonly slots: readonly Slot[];
};

// Zones that stack down from the sidebar's top, and ones that stack up from its bottom (the
// instance list between them absorbs the difference), each in screen order.
const fromTop: SidebarZone[] = ["top", "middleTop"];
const fromBottom: SidebarZone[] = ["middleBottom", "bottom"];

const DRAG_THRESHOLD = 4;
const DRAG_BODY_CLASS = "reorder-dragging";

// Drag a tab between the sidebar's zones (`[data-zone]` holding `[data-tab]` slots). It snaps to
// the nearest place it could land, measured once with the tab lifted out, so the preview
// shifting things around never moves the targets under the pointer.
export function createTabDrag(options: {
	container: () => HTMLElement | undefined;
	onDrop: (id: SidebarTab, target: DropTarget) => void;
}) {
	let drag = $state<TabDragState | null>(null);
	let target = $state<DropTarget | null>(null);
	let pointerX = $state(0);
	let pointerY = $state(0);
	let pending: { id: SidebarTab; startX: number; startY: number } | null = null;

	// `grab` is where the tab was pressed, so it stays under the pointer where it was picked up.
	function measure(id: SidebarTab, grab: { x: number; y: number }): TabDragState | null {
		const root = options.container();
		if (!root) return null;
		const zones = new Map<SidebarZone, { rect: DOMRect; centers: number[]; gap: number }>();
		let from: DropTarget | undefined;
		let rect: DOMRect | undefined;
		for (const el of root.querySelectorAll<HTMLElement>("[data-zone]")) {
			const zone = el.dataset.zone as SidebarZone;
			const centers: number[] = [];
			const tabs = [...el.querySelectorAll<HTMLElement>(":scope > [data-tab]")];
			for (const [index, slot] of tabs.entries()) {
				const r = slot.getBoundingClientRect();
				if (slot.dataset.tab === id) {
					from = { zone, index };
					rect = r;
				}
				centers.push(r.top + r.height / 2);
			}
			zones.set(zone, {
				rect: el.getBoundingClientRect(),
				centers,
				gap: Number.parseFloat(getComputedStyle(el).rowGap) || 0,
			});
		}
		if (!from || !rect) return null;

		const lifted = from;
		const pitch = rect.height + (zones.get(lifted.zone)?.gap ?? 0);
		const half = rect.height / 2;
		const slots: Slot[] = [];
		for (const [zone, { rect: zoneRect, centers }] of zones) {
			const stacksDown = fromTop.includes(zone);
			const flow = stacksDown ? fromTop : fromBottom;
			// Further from where the zone's flow is anchored
			const behind = (a: number, b: number) => (stacksDown ? a > b : a < b);
			// Lifting the tab out closes its gap: what stacks behind it in the same flow moves in.
			const closeUp = stacksDown ? -pitch : pitch;
			const liftedAt = flow.indexOf(lifted.zone);
			const zoneShift = liftedAt >= 0 && behind(flow.indexOf(zone), liftedAt) ? closeUp : 0;
			const own = zone === lifted.zone;
			const rest = centers.flatMap((y, i) => {
				if (!own) return [y + zoneShift];
				if (i === lifted.index) return [];
				return [behind(i, lifted.index) ? y + closeUp : y];
			});
			const n = rest.length;
			for (let index = 0; index <= n; index++) {
				// Down-stacking zones push what follows the new tab down; up-stacking ones push
				// what precedes it up.
				const y = stacksDown
					? index < n
						? rest[index]!
						: n > 0
							? rest[n - 1]! + pitch
							: zoneRect.top + zoneShift + half
					: index < n
						? rest[index]! - pitch
						: zoneRect.bottom + zoneShift - half;
				slots.push({ zone, index, y });
			}
		}
		return {
			id,
			from: lifted,
			offsetX: grab.x - rect.left,
			offsetY: grab.y - rect.top,
			height: rect.height,
			slots,
		};
	}

	function nearest(state: TabDragState, clientY: number): DropTarget {
		const centre = clientY - state.offsetY + state.height / 2;
		let best = state.slots[0]!;
		for (const slot of state.slots) {
			if (Math.abs(slot.y - centre) < Math.abs(best.y - centre)) best = slot;
		}
		return { zone: best.zone, index: best.index };
	}

	function onPointerMove(e: PointerEvent) {
		if (!pending) return;
		if (!drag) {
			if (
				Math.abs(e.clientY - pending.startY) < DRAG_THRESHOLD &&
				Math.abs(e.clientX - pending.startX) < DRAG_THRESHOLD
			) {
				return;
			}
			drag = measure(pending.id, { x: pending.startX, y: pending.startY });
			if (!drag) return;
		}
		pointerX = e.clientX;
		pointerY = e.clientY;
		target = nearest(drag, e.clientY);
	}

	function end(commit: boolean) {
		window.removeEventListener("pointermove", onPointerMove);
		window.removeEventListener("pointerup", onPointerUp);
		window.removeEventListener("keydown", onKey);
		if (drag) {
			const { id, from } = drag;
			if (commit && target && (target.zone !== from.zone || target.index !== from.index)) {
				options.onDrop(id, target);
			}
			// The click that follows a drag would navigate. It doesn't always come (released off
			// the tab), so the guard also expires on its own.
			const suppressClick = (ev: MouseEvent) => {
				ev.preventDefault();
				ev.stopPropagation();
			};
			window.addEventListener("click", suppressClick, { capture: true, once: true });
			window.setTimeout(
				() => window.removeEventListener("click", suppressClick, { capture: true }),
				500,
			);
			if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
		}
		pending = null;
		drag = null;
		target = null;
	}

	const onPointerUp = () => end(true);
	function onKey(e: KeyboardEvent) {
		if (e.key === "Escape" && drag) {
			e.preventDefault();
			end(false);
		}
	}

	function pointerdown(id: SidebarTab) {
		return (e: PointerEvent) => {
			if (e.button !== 0) return;
			pending = { id, startX: e.clientX, startY: e.clientY };
			window.addEventListener("pointermove", onPointerMove);
			window.addEventListener("pointerup", onPointerUp, { once: true });
			window.addEventListener("keydown", onKey);
		};
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
		get target() {
			return target;
		},
		get pointerX() {
			return pointerX;
		},
		get pointerY() {
			return pointerY;
		},
		pointerdown,
	};
}
