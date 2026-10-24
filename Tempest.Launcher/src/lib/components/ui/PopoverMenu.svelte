<script lang="ts">
	import { Popover } from "bits-ui";
	import type { Snippet } from "svelte";

	interface Props {
		trigger: Snippet;
		children: Snippet;
		align?: "start" | "end" | "center";
		class?: string;
		triggerClass?: string;
		triggerLabel?: string;
		/** Width of the item list, e.g. wider for long labels. */
		menuClass?: string;
		/** Bound by menus whose items should close it; it stays open on a click otherwise. */
		open?: boolean;
	}

	let {
		trigger,
		children,
		align = "end",
		class: className = "",
		triggerClass = "",
		triggerLabel,
		menuClass = "w-52",
		open = $bindable(false),
	}: Props = $props();
</script>

<Popover.Root bind:open>
	<Popover.Trigger
		class="list-none {triggerClass}"
		aria-label={triggerLabel}
		title={triggerLabel}
		onclick={(e) => e.stopPropagation()}
	>
		{@render trigger()}
	</Popover.Trigger>
	<Popover.Portal>
		<Popover.Content
			data-popover-menu
			class="bg-base-300 rounded-box border-base-content/10 popover-animate z-50 min-w-[180px] border p-1 shadow-xl {className}"
			{align}
			sideOffset={4}
		>
			<ul role="menu" class="menu {menuClass}">
				{@render children()}
			</ul>
		</Popover.Content>
	</Popover.Portal>
</Popover.Root>
