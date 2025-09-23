<script lang="ts">
	import { Check, ChevronDown, ChevronUp } from "@lucide/svelte";
	import { Select, type WithoutChildren } from "bits-ui";

	type Item = { value: any; label: string; disabled?: boolean; group?: string; data?: any };

	type Props = WithoutChildren<Select.RootProps> & {
		placeholder?: string;
		items: Item[];
		contentProps?: WithoutChildren<Select.ContentProps>;
	};

	let {
		value = $bindable(),
		items,
		contentProps,
		placeholder = "Select an option",
		...restProps
	}: Props = $props();

	let open = $state(false);

	const selectedLabel = $derived(items.find((i) => i.value === value)?.label ?? "");

	const grouped = $derived.by(() => {
		const map: Record<string, Item[]> = {};
		for (const it of items) {
			const g = it.group || "__ungrouped";
			(map[g] ||= []).push(it);
		}
		return Object.entries(map);
	});
</script>

<Select.Root bind:value={value as never} bind:open {...restProps}>
	<Select.Trigger
		class="bg-background-800 focus-visible:ring-primary-500 select-trigger flex h-10 w-full min-w-[220px] items-center gap-2 rounded-lg px-3 text-sm transition duration-150 hover:cursor-pointer hover:border-transparent hover:brightness-90 focus-visible:ring-2 focus-visible:outline-none"
	>
		<span class="text-text-color truncate" class:list={[!selectedLabel && "opacity-60"]}>
			{selectedLabel || placeholder}
		</span>
		<span
			class="text-text-color ml-auto transition-transform duration-150"
			class:list={[open && "rotate-180"]}
		>
			<ChevronDown
				class={open ?
					"size-4 shrink-0 rotate-180 transition duration-150"
				:	"size-4 shrink-0 transition duration-150"}
			/>
		</span>
	</Select.Trigger>

	{#if open}
		<Select.Portal>
			<Select.Content
				side={contentProps?.side ?? "bottom"}
				class="bg-background-800 z-50 max-h-[var(--bits-select-content-available-height)] w-[var(--bits-select-anchor-width)] min-w-[var(--bits-select-anchor-width)] rounded-xl px-1 py-2 outline-hidden select-none data-[side=bottom]:translate-y-1 data-[side=left]:-translate-x-1 data-[side=right]:translate-x-1 data-[side=top]:-translate-y-1"
				{...contentProps}
			>
				<Select.ScrollUpButton
					class="flex items-center justify-center py-1 text-xs opacity-60"
				>
					<ChevronUp class="size-4" />
				</Select.ScrollUpButton>
				<Select.Viewport>
					{#if grouped.length === 0}
						<div class="text-text-color px-4 py-2 opacity-50">No options</div>
					{:else}
						{#each grouped as [groupName, groupItems]}
							<Select.Group>
								{#if groupName !== "__ungrouped"}
									<Select.GroupHeading
										class="text-text-color py-1 pr-4 pl-4 font-semibold capitalize"
										>{groupName}</Select.GroupHeading
									>
								{/if}
								{#each groupItems as item (item.value)}
									<Select.Item
										value={item.value}
										label={item.label}
										disabled={item.disabled}
									>
										{#snippet child({ props, selected, highlighted })}
											<div
												{...props}
												class={`relative flex h-10 w-full items-center rounded-md pr-2 pl-5 text-sm capitalize transition-colors outline-none select-none hover:cursor-pointer
												${
													highlighted ?
														"bg-secondary-800 text-primary-300"
													: selected ? "bg-secondary-800 text-primary-300"
													: "text-text-color"
												}`}
											>
												<span>{item.label}</span>
												{#if selected}
													<span class="text-primary ml-auto"
														><Check class="size-4" /></span
													>
												{/if}
											</div>
										{/snippet}
									</Select.Item>
								{/each}
							</Select.Group>
						{/each}
					{/if}
				</Select.Viewport>
				<Select.ScrollDownButton
					class="flex items-center justify-center py-1 text-xs opacity-60"
				>
					<ChevronDown class="size-4" />
				</Select.ScrollDownButton>
			</Select.Content>
		</Select.Portal>
	{/if}
</Select.Root>

<style>
	:global(.dropdown-menu) {
		scrollbar-width: thin;
		scrollbar-color: #428eb1 #222329;
	}
	:global(.dropdown-menu)::-webkit-scrollbar {
		width: 8px;
	}
	:global(.dropdown-menu)::-webkit-scrollbar-track {
		background: #222329;
		border-radius: 4px;
	}
	:global(.dropdown-menu)::-webkit-scrollbar-thumb {
		background: #428eb1;
		border-radius: 4px;
		border: 1px solid #222329;
	}
	:global(.dropdown-menu)::-webkit-scrollbar-thumb:hover {
		background: #7fa7be;
	}
</style>
