<script lang="ts">
	import { page } from "$app/state";
	import { getContrastColor } from "$lib/utils/color";
	import { Tooltip } from "bits-ui";
	import type { Component } from "svelte";

	interface Props {
		href: string;
		icon: Component<any>;
		label: string;
		active?: boolean;
		circle?: boolean;
		color?: string;
		onpointerdown?: (e: PointerEvent) => void;
	}

	let { href, icon: Icon, label, active, circle, color, onpointerdown }: Props = $props();

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
					`background-color: ${isActive ? color : 'transparent'}; border-color: ${isActive ? color : 'transparent'}; color: ${isActive ? getContrastColor(color) : color};`
				:	''}
				aria-label={label}
			>
				<Icon size={20} />
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

