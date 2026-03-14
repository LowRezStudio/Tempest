<script lang="ts" generics="T extends string">
	import type { Snippet } from "svelte";

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
	}: Props<T> = $props();
</script>

<div class="bg-base-200 {className}">
	<div class="px-4 py-3">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-3">
				<div
					class="w-16 h-16 rounded-xl bg-base-300 flex items-center justify-center shrink-0"
				>
					{@render icon()}
				</div>
				<div>
					<h1 class="text-2xl font-bold mb-1">
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
		<div role="tablist" class="tabs tabs-border">
			{#each tabs as tab (tab.value)}
				<button
					role="tab"
					aria-selected={activeTab === tab.value}
					class={activeTab === tab.value ? "tab tab-active" : "tab"}
					onclick={() => onSelectTab?.(tab.value)}
				>
					{tab.name}
				</button>
			{/each}
		</div>
	</div>
</div>
