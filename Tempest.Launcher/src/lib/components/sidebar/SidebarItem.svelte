<script lang="ts">
	import { page } from "$app/state";
	import { getContrastColor } from "$lib/utils/color";
	import { Tooltip } from "bits-ui";
	import type { Snippet } from "svelte";

	interface Props {
		href: string;
		children: Snippet;
		label: string;
		active?: boolean;
		circle?: boolean;
		color?: string;
		onpointerdown?: (e: PointerEvent) => void;
	}

	let { href, children, label, active, circle, color, onpointerdown }: Props = $props();

	let isActive = $derived(active ?? page.url.pathname === href);
</script>

<Tooltip.Root delayDuration={150} disableHoverableContent>
	<Tooltip.Trigger>
		{#snippet child({ props })}
			<a
				{...props}
				{href}
				{onpointerdown}
				class:rounded-full={circle}
				class="btn btn-square"
				class:btn-accent={isActive && !color}
				class:btn-ghost={!isActive && !color}
				style={color ?
					`background-color: ${isActive ? color : `color-mix(in srgb, ${color} 15%, transparent)`}; border-color: ${isActive ? 'white' : color}; color: ${isActive ? getContrastColor(color) : color};`
				:	''}
				aria-label={label}
			>
				{@render children()}
			</a>
		{/snippet}
	</Tooltip.Trigger>
	<Tooltip.Portal>
		<Tooltip.Content
			side="right"
			sideOffset={8}
			class="z-50 bg-neutral text-neutral-content text-xs font-semibold px-2.5 py-1.5 rounded-lg shadow-md transition-opacity duration-100 data-[state=closed]:opacity-0 data-[state=open]:opacity-100"
		>
			<Tooltip.Arrow class="fill-neutral" />
			{label}
		</Tooltip.Content>
	</Tooltip.Portal>
</Tooltip.Root>
