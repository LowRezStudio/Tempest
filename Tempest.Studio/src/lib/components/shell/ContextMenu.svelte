<!--
	The window's context menu (`ui.openContextMenu`): opens at the pointer,
	stays inside the window, closes on Escape, an outside click, scroll or
	resize. Up/Down move between items.
-->
<script lang="ts">
	import { tick } from "svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import MenuItems from "./MenuItems.svelte";

	let el = $state<HTMLUListElement | null>(null);
	let pos = $state({ x: 0, y: 0 });

	$effect(() => {
		const menu = ui.contextMenu;
		if (!menu) return;
		pos = { x: menu.x, y: menu.y };
		void tick().then(() => {
			if (!el) return;
			const r = el.getBoundingClientRect();
			const margin = 4;
			pos = {
				x: Math.max(margin, Math.min(menu.x, window.innerWidth - r.width - margin)),
				y:
					menu.y + r.height + margin > window.innerHeight
						? Math.max(margin, menu.y - r.height)
						: menu.y,
			};
			el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
		});
	});

	function close() {
		ui.contextMenu = null;
	}

	function run(fn: () => unknown) {
		close();
		void fn();
	}

	function onKey(event: KeyboardEvent) {
		if (!ui.contextMenu) return;
		if (event.key === "Escape") {
			event.preventDefault();
			close();
			return;
		}
		if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
		event.preventDefault();
		const buttons = [
			...(el?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []),
		];
		if (buttons.length === 0) return;
		const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
		const next =
			event.key === "ArrowDown"
				? buttons[(at + 1) % buttons.length]
				: buttons[(at - 1 + buttons.length) % buttons.length];
		next?.focus();
	}

	function onPointer(event: MouseEvent) {
		if (ui.contextMenu && el && !el.contains(event.target as Node)) close();
	}
</script>

<svelte:window
	onkeydowncapture={onKey}
	onmousedowncapture={onPointer}
	onresize={close}
	onblur={close}
/>
<svelte:document onscrollcapture={() => ui.contextMenu && close()} />

{#if ui.contextMenu}
	<ul
		bind:this={el}
		class="menu menu-sm bg-base-200 border-base-300 rounded-box fixed z-[100] w-72 border p-1 shadow-xl"
		style="left: {pos.x}px; top: {pos.y}px"
		role="menu"
		oncontextmenu={(e) => e.preventDefault()}
	>
		<MenuItems items={ui.contextMenu.items} onrun={run} />
	</ul>
{/if}
