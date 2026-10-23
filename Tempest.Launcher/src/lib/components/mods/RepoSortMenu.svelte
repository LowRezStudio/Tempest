<script lang="ts">
	import { ArrowDownAZ, Check, ChevronDown, Clock, Download, Sparkles } from "@lucide/svelte";
	import { DropdownMenu } from "bits-ui";
	import { sorts } from "$lib/mods/browse.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { RepoSort } from "$lib/queries/repo";

	let { value = $bindable() }: { value: RepoSort } = $props();

	let open = $state(false);

	const look: Record<RepoSort, { icon: typeof Clock; label: () => string; hint: () => string }> =
		{
			downloads: {
				icon: Download,
				label: m.repo_sort_downloads,
				hint: m.repo_sort_downloads_hint,
			},
			updated: { icon: Clock, label: m.repo_sort_updated, hint: m.repo_sort_updated_hint },
			created: { icon: Sparkles, label: m.repo_sort_created, hint: m.repo_sort_created_hint },
			name: { icon: ArrowDownAZ, label: m.repo_sort_name, hint: m.repo_sort_name_hint },
		};
	const CurrentIcon = $derived(look[value].icon);
</script>

<DropdownMenu.Root bind:open>
	<DropdownMenu.Trigger
		class="btn btn-sm shrink-0 flex-nowrap gap-1.5 font-normal {open ? 'btn-active' : ''}"
		aria-label={m.repo_sort_by()}
	>
		<CurrentIcon size={14} class="opacity-70" />
		{look[value].label()}
		<ChevronDown size={14} class="opacity-60 transition-transform {open ? 'rotate-180' : ''}" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Portal>
		<DropdownMenu.Content
			data-popover-menu
			class="bg-base-200 border-base-300 rounded-box popover-animate z-50 flex w-64 flex-col gap-0.5 border p-1.5 shadow-xl outline-none"
			align="end"
			sideOffset={4}
		>
			<span class="text-base-content/50 px-2 pt-1 pb-1.5 text-xs font-semibold">
				{m.repo_sort_by()}
			</span>
			<DropdownMenu.RadioGroup bind:value={() => value, (v) => (value = v as RepoSort)}>
				{#each sorts as sort (sort)}
					{@const Icon = look[sort].icon}
					<DropdownMenu.RadioItem
						value={sort}
						class="rounded-field data-highlighted:bg-base-300 flex cursor-pointer items-start gap-2.5 px-2 py-1.5 text-sm outline-none"
					>
						{#snippet children({ checked })}
							<Icon size={14} class="mt-0.5 shrink-0 opacity-70" />
							<span class="flex min-w-0 flex-1 flex-col">
								<span class:font-semibold={checked}>{look[sort].label()}</span>
								<span class="text-base-content/60 text-xs">{look[sort].hint()}</span
								>
							</span>
							{#if checked}
								<Check size={14} class="text-primary mt-0.5 shrink-0" />
							{/if}
						{/snippet}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
		</DropdownMenu.Content>
	</DropdownMenu.Portal>
</DropdownMenu.Root>
