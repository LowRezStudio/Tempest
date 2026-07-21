<script lang="ts">
	import { page } from "$app/state";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { Tooltip } from "bits-ui";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		href: string;
		active?: boolean;
		onpointerdown?: (e: PointerEvent) => void;
	}

	let { instance, href, active, onpointerdown }: Props = $props();

	let isActive = $derived(active ?? page.url.pathname === href);
	let color = $derived(getInstanceColor(instance));
	let muted = $derived(`color-mix(in oklab, ${color} 45%, var(--color-base-content))`);
	let label = $derived(instance.label?.trim() || "?");
	let version = $derived(instance.version?.trim() || "");

	let style = $derived(
		isActive
			? `background-color: ${muted}; color: ${getContrastColor(color)};`
			: `color: ${muted};`,
	);
</script>

<Tooltip.Root delayDuration={150} disableHoverableContent>
	<Tooltip.Trigger>
		{#snippet child({ props })}
			<a
				{...props}
				{href}
				{onpointerdown}
				class="btn btn-square"
				class:btn-active={isActive}
				style={style}
				aria-label={instance.label}
			>
				<span
				class="max-w-full truncate px-0.5 text-[10px] font-bold leading-none tracking-tight tabular-nums"
			>
				{#if version}
					{version}
				{:else}
					{(label[0] ?? "?").toUpperCase()}
				{/if}
			</span>
			</a>
		{/snippet}
	</Tooltip.Trigger>
	<Tooltip.Portal>
		<Tooltip.Content
			side="right"
			sideOffset={8}
			class="z-50 flex items-center gap-1.5 rounded-lg bg-neutral px-2.5 py-1.5 text-xs font-semibold text-neutral-content shadow-md transition-opacity duration-100 data-[state=closed]:opacity-0 data-[state=open]:opacity-100"
		>
			<Tooltip.Arrow class="fill-neutral" />
			<span>{instance.label}</span>
			{#if instance.version}
				<span class="opacity-60">· v{instance.version}</span>
			{/if}
		</Tooltip.Content>
	</Tooltip.Portal>
</Tooltip.Root>
