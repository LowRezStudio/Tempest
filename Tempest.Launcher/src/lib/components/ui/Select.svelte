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

	const selectedLabel = $derived(items.find(i => i.value === value)?.label ?? "");

	const grouped = $derived.by(() => {
		const map: Record<string, Item[]> = {};
		for (const it of items) {
			const g = it.group || "__ungrouped";
			(map[g] ||= []).push(it);
		}
		return Object.entries(map);
	});
</script>

<Select.Root
	bind:value={value as never}
	bind:open={open}
	{...restProps}
>
	<Select.Trigger
		class="h-10 rounded-lg bg-component-background data-placeholder:text-foreground-alt/50 flex w-full min-w-[220px] items-center px-3 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/60 gap-2 select-trigger"
	>
		<span class="truncate text-text-color" class:list={[!selectedLabel && "opacity-60"]}>
			{selectedLabel || placeholder}
		</span>
		<span
			class="ml-auto text-text-color transition-transform duration-150"
			class:list={[open && "rotate-180"]}
		>
			<ChevronDown class={open ? "size-4 shrink-0 rotate-180 transition duration-150": "size-4 shrink-0 transition duration-150"} />
		</span>
	</Select.Trigger>

	{#if open}
		<Select.Portal>
			<Select.Content
				side={contentProps?.side ?? "bottom"}
				class="focus-override bg-component-background data-[side=bottom]:translate-y-1 data-[side=top]:-translate-y-1 data-[side=left]:-translate-x-1 data-[side=right]:translate-x-1 outline-hidden z-50 max-h-[var(--bits-select-content-available-height)] w-[var(--bits-select-anchor-width)] min-w-[var(--bits-select-anchor-width)] select-none rounded-xl px-1 py-2"
				{...contentProps}
			>
				<Select.ScrollUpButton class="flex items-center justify-center py-1 text-xs opacity-60">
					<ChevronUp class="size-4" />
				</Select.ScrollUpButton>
				<Select.Viewport>
					{#if grouped.length === 0}
						<div class="py-2 px-4 text-text-color opacity-50">No options</div>
					{:else}
						{#each grouped as [groupName, groupItems]}
							<Select.Group>
								{#if groupName !== "__ungrouped"}
									<Select.GroupHeading class="py-1 pl-4 pr-4 font-semibold capitalize text-text-color">{
										groupName
									}</Select.GroupHeading>
								{/if}
								{#each groupItems as item (item.value)}
									<Select.Item value={item.value} label={item.label} disabled={item.disabled}>
										{#snippet child({ props, selected, highlighted })}
											<div
												{...props}
												class={`relative flex h-10 w-full select-none items-center rounded-md pl-5 pr-2 text-sm capitalize outline-none transition-colors
												${
													highlighted
														? "bg-secondary-800 text-primary-300"
														: selected
														? "bg-secondary-800 text-primary-300"
														: "text-text-color"
												}`}
											>
												<span>{item.label}</span>
												{#if selected}
													<span class="ml-auto text-primary"><Check class="size-4" /></span>
												{/if}
											</div>
										{/snippet}
									</Select.Item>
								{/each}
							</Select.Group>
						{/each}
					{/if}
				</Select.Viewport>
				<Select.ScrollDownButton class="flex items-center justify-center py-1 text-xs opacity-60">
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
