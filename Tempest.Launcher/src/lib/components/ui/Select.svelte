<script lang="ts">
	import { Check, ChevronDown } from "@lucide/svelte";
	import { createSelect, melt } from "@melt-ui/svelte";
	import { fade } from "svelte/transition";

	interface Props {
		choices: Record<string, string[]>;
		placeholder?: string;
		value?: string;
		direction?: "bottom" | "top";
		fitViewport?: boolean;
		onValueChange?: (value: string | undefined) => void;
	}

	let {
		choices = {},
		placeholder = "Select an option",
		value = $bindable<string | undefined>(),
		fitViewport,
		direction = "bottom",
		onValueChange,
	}: Props = $props();

	let search = $state("");
	let isSearching = $state(false);
	let inputElement: HTMLInputElement;

	const {
		elements: { trigger, menu, option, group, groupLabel },
		states: { open },
		helpers: { isSelected },
	} = createSelect<string>({
		forceVisible: true,
		positioning: {
			placement: direction,
			fitViewport,
			sameWidth: true,
		},
		defaultSelected: value ? { value, label: value } : undefined,
		onSelectedChange: ({ next }) => {
			value = next?.value;
			onValueChange?.(next?.value);
			search = "";
			isSearching = false;
			return next;
		},
		onOpenChange: ({ next }) => {
			if (next) {
				// When opening: clear search to show all options
				search = "";
				isSearching = true;
			} else {
				// When closing: blur input and reset search state
				search = "";
				isSearching = false;
				if (inputElement) {
					inputElement.blur();
				}
			}
			return next;
		},
	});

	const filteredChoices = $derived.by(() => {
		if (!search.trim()) return choices;
		const lower = search.toLowerCase();
		const filtered: Record<string, string[]> = {};
		for (const [groupName, arr] of Object.entries(choices)) {
			const match = arr.filter((item) => item.toLowerCase().includes(lower));
			if (match.length) filtered[groupName] = match;
		}
		return filtered;
	});

	const displayValue = $derived(isSearching ? search : value || "");
	const displayPlaceholder = $derived(value && !isSearching ? "" : placeholder);
</script>

<div class="relative flex flex-col gap-1">
	<div class="relative">
		<button
			class="flex h-10 w-full min-w-[220px] items-center justify-between rounded-lg bg-component-background px-3 py-2 shadow transition-opacity hover:opacity-90"
			use:melt={$trigger}
		>
			<span class="sr-only">Open select</span>
		</button>

		<input
			bind:this={inputElement}
			type="text"
			class="absolute inset-0 h-full w-full bg-transparent px-3 py-2 text-text-color outline-none"
			placeholder={displayPlaceholder}
			value={displayValue}
			oninput={(e) => {
				search = (e.target as HTMLInputElement).value;
				isSearching = true;
				if (!$open) open.set(true);
			}}
			onfocus={() => {
				if (!$open) open.set(true);
			}}
			onblur={() => {
				// Only reset if dropdown is closed
				if (!$open && !value) {
					search = "";
					isSearching = false;
				}
			}}
		/>

		<ChevronDown
			class={`absolute right-3 top-1/2 size-4 -translate-y-1/2 transition-transform duration-150 text-text-color pointer-events-none ${
				$open ? "rotate-180" : ""
			}`}
		/>
	</div>

	{#if $open}
		<div
			class="dropdown-menu absolute z-50 flex max-h-[245px] min-w-[220px] flex-col
			overflow-y-auto rounded-lg bg-component-background p-1 mt-1
			shadow-lg focus:!ring-0"
			use:melt={$menu}
			transition:fade={{ duration: 150 }}
		>
			{#if Object.keys(filteredChoices).length === 0}
				<div class="py-2 px-4 text-text-color opacity-50">No results</div>
			{:else}
				{#each Object.entries(filteredChoices) as [key, arr]}
					<div use:melt={$group(key)}>
						<div
							class="py-1 pl-4 pr-4 font-semibold capitalize text-text-color"
							use:melt={$groupLabel(key)}
						>
							{key}
						</div>
						{#each arr as item}
							<div
								class="relative cursor-pointer rounded-lg py-1 pl-8 pr-4 text-text-color
								hover:bg-magnum-100 focus:z-10
								focus:text-magnum-700
								data-[highlighted]:bg-magnum-200 data-[highlighted]:text-magnum-900
								data-[disabled]:opacity-50"
								use:melt={$option({ value: item, label: item })}
							>
								<div class="check {$isSelected(item) ? 'block' : 'hidden'}">
									<Check class="size-4" />
								</div>
								{item}
							</div>
						{/each}
					</div>
				{/each}
			{/if}
		</div>
	{/if}
</div>

<style>
	.check {
		position: absolute;
		left: theme(spacing.2);
		top: 50%;
		z-index: theme(zIndex.20);
		translate: 0 calc(-50% + 1px);
		color: var(--color-primary);
	}

	/* Custom scrollbar styling */
	.dropdown-menu {
		scrollbar-width: thin;
		scrollbar-color: #428eb1 #222329;
	}

	.dropdown-menu::-webkit-scrollbar {
		width: 8px;
	}

	.dropdown-menu::-webkit-scrollbar-track {
		background: #222329;
		border-radius: 4px;
	}

	.dropdown-menu::-webkit-scrollbar-thumb {
		background: #428eb1;
		border-radius: 4px;
		border: 1px solid #222329;
	}

	.dropdown-menu::-webkit-scrollbar-thumb:hover {
		background: #7fa7be;
	}
</style>
