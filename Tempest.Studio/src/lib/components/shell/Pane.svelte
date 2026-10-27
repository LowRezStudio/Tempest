<!--
	A resizable side pane. The handle sits on the edge facing the editor: drag
	to resize, double-click to reset, arrow keys when focused. Sizes are
	remembered per pane id. With `overlay` (narrow windows) the pane floats
	over the editor instead of taking room.
-->
<script lang="ts">
	import { ui } from "$lib/studio/ui.svelte";
	import type { Snippet } from "svelte";

	interface Props {
		id: string;
		/** Which side of the editor the pane is on. */
		side: "left" | "right";
		initial: number;
		min?: number;
		max?: number;
		overlay?: boolean;
		class?: string;
		children: Snippet;
	}
	const {
		id,
		side,
		initial,
		min = 160,
		max = 900,
		overlay = false,
		class: cls = "",
		children,
	}: Props = $props();

	const width = $derived(
		Math.min(Math.max(ui.size(id, initial), min), Math.max(min, Math.min(max, ui.width * 0.6))),
	);
	let dragging = $state(false);

	function onPointerDown(event: PointerEvent) {
		event.preventDefault();
		const start = event.clientX;
		const from = width;
		dragging = true;
		const el = event.currentTarget as HTMLElement;
		el.setPointerCapture(event.pointerId);
		const move = (e: PointerEvent) => {
			const dx = e.clientX - start;
			ui.setSize(id, Math.min(max, Math.max(min, from + (side === "left" ? dx : -dx))));
		};
		const up = () => {
			dragging = false;
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	function onKeydown(event: KeyboardEvent) {
		const step = event.shiftKey ? 48 : 16;
		if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
			event.preventDefault();
			const grow = (event.key === "ArrowRight") === (side === "left");
			ui.setSize(id, Math.min(max, Math.max(min, width + (grow ? step : -step))));
		}
	}
</script>

<div
	class="bg-base-100 relative flex min-h-0 shrink-0 flex-col {side === 'left'
		? 'border-base-300 border-r'
		: 'border-base-300 border-l'} {overlay
		? `absolute inset-y-0 z-30 shadow-2xl ${side === 'left' ? 'left-0' : 'right-0'}`
		: ''} {cls}"
	style:width="{width}px"
>
	{@render children()}
	<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
	<div
		class="group absolute inset-y-0 z-10 w-1.5 cursor-col-resize {side === 'left'
			? '-right-1'
			: '-left-1'}"
		role="separator"
		aria-orientation="vertical"
		aria-label="Resize"
		aria-valuenow={width}
		tabindex="0"
		onpointerdown={onPointerDown}
		ondblclick={() => ui.setSize(id, initial)}
		onkeydown={onKeydown}
		title="Drag to resize, double-click to reset"
	>
		<div
			class="mx-auto h-full w-px transition-colors {dragging
				? 'bg-primary'
				: 'group-hover:bg-primary/60 group-focus-visible:bg-primary'}"
		></div>
	</div>
</div>
{#if dragging}
	<!-- Keeps the resize cursor and stops canvases from eating the drag. -->
	<div class="fixed inset-0 z-50 cursor-col-resize"></div>
{/if}
