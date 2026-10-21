<script lang="ts">
	import {
		ArrowDownAZ,
		ArrowDownWideNarrow,
		ArrowUpDown,
		ArrowUpNarrowWide,
		Check,
		ChevronDown,
	} from "@lucide/svelte";
	import { DropdownMenu } from "bits-ui";
	import { m } from "$lib/paraglide/messages";
	import {
		arrangeInstances,
		arrangements,
		currentArrangement,
		type Arrangement,
	} from "$lib/stores/instance.svelte";

	let open = $state(false);

	const look: Record<
		Arrangement,
		{ icon: typeof ArrowUpDown; label: () => string; hint: () => string }
	> = {
		patch: {
			icon: ArrowUpNarrowWide,
			label: m.library_arrange_patch,
			hint: m.library_arrange_patch_hint,
		},
		"patch-desc": {
			icon: ArrowDownWideNarrow,
			label: m.library_arrange_patch_desc,
			hint: m.library_arrange_patch_desc_hint,
		},
		name: {
			icon: ArrowDownAZ,
			label: m.library_arrange_name,
			hint: m.library_arrange_name_hint,
		},
	};
	const current = $derived(currentArrangement.value);
	const CurrentIcon = $derived(current ? look[current].icon : ArrowUpDown);
</script>

<DropdownMenu.Root bind:open>
	<DropdownMenu.Trigger
		class="btn btn-sm shrink-0 flex-nowrap gap-1.5 font-normal {open ? 'btn-active' : ''}"
		aria-label={m.library_arrange()}
	>
		<CurrentIcon size={14} class="opacity-70" />
		{current ? look[current].label() : m.library_arrange_custom()}
		<ChevronDown size={14} class="opacity-60 transition-transform {open ? 'rotate-180' : ''}" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Portal>
		<DropdownMenu.Content
			class="bg-base-200 border-base-300 rounded-box popover-animate z-50 flex w-64 flex-col gap-0.5 border p-1.5 shadow-xl outline-none"
			align="end"
			sideOffset={4}
		>
			<span class="text-base-content/50 px-2 pt-1 pb-1.5 text-xs font-semibold">
				{m.library_arrange()}
			</span>
			<DropdownMenu.RadioGroup
				bind:value={() => current ?? "", (v) => arrangeInstances(v as Arrangement)}
			>
				{#each arrangements as by (by)}
					{@const Icon = look[by].icon}
					<DropdownMenu.RadioItem
						value={by}
						class="rounded-field data-highlighted:bg-base-300 flex cursor-pointer items-start gap-2.5 px-2 py-1.5 text-sm outline-none"
					>
						{#snippet children({ checked })}
							<Icon size={14} class="mt-0.5 shrink-0 opacity-70" />
							<span class="flex min-w-0 flex-1 flex-col">
								<span class:font-semibold={checked}>{look[by].label()}</span>
								<span class="text-base-content/60 text-xs">{look[by].hint()}</span>
							</span>
							{#if checked}
								<Check size={14} class="text-primary mt-0.5 shrink-0" />
							{/if}
						{/snippet}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
			<p class="text-base-content/50 border-base-300 mt-1 border-t px-2 pt-2 pb-1 text-xs">
				{m.library_arrange_note()}
			</p>
		</DropdownMenu.Content>
	</DropdownMenu.Portal>
</DropdownMenu.Root>
