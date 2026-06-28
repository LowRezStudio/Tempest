<script lang="ts" generics="T extends string">
	import type { Snippet } from "svelte";
	import { Tabs } from "bits-ui";

	interface Props<Tab> {
		title: string;
		subtitle?: Snippet;
		icon: Snippet;
		actions?: Snippet;
		errors?: Snippet;
		activeTab?: Tab;
		onSelectTab?: (tab: Tab) => void;
		tabs?: { name: string; value: Tab }[];
		class?: string;
		iconBg?: string;
	}

	let {
		title,
		subtitle,
		icon,
		actions,
		errors,
		activeTab,
		onSelectTab,
		tabs,
		class: className = "",
		iconBg,
	}: Props<T> = $props();
</script>

<div class="bg-base-200 {className}">
	<div class="px-4 py-3">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-3">
				<div
					class="w-16 h-16 rounded-xl flex items-center justify-center shrink-0"
					style={iconBg ? `background-color: ${iconBg};` : ""}
					class:bg-base-300={!iconBg}
				>
					{@render icon()}
				</div>
				<div>
					<h1 class="text-2xl font-bold mb-1 uppercase">
						{title}
					</h1>
					{#if subtitle}
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<div class="flex items-center gap-1.5">
								{@render subtitle()}
							</div>
						</div>
					{/if}
				</div>
			</div>
			{#if actions}
				<div class="flex items-center gap-2">
					{@render actions()}
				</div>
			{/if}
		</div>
		{@render errors?.()}
	</div>
	<div class="px-4">
		{#if tabs && tabs.length > 0}
			<Tabs.Root
				value={activeTab}
				onValueChange={(val) => onSelectTab?.(val as T)}
			>
				<Tabs.List class="tabs tabs-border">
					{#each tabs as tab (tab.value)}
						<Tabs.Trigger
							value={tab.value}
							class="tab data-[state=active]:tab-active"
						>
							{tab.name}
						</Tabs.Trigger>
					{/each}
				</Tabs.List>
			</Tabs.Root>
		{/if}
	</div>
</div>
