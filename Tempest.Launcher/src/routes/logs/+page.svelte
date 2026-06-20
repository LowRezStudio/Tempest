<script lang="ts">
	import { ScrollText, Trash2 } from "@lucide/svelte";
	import GhosttyTerminal from "$lib/components/ui/GhosttyTerminal.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { clearProcessLogs, processLogs } from "$lib/stores/processes";

	let activeSource = $state("all");

	const uniqueSources = $derived.by(() => {
		const seen = new Set<string>();
		const sources: string[] = [];
		for (const log of $processLogs) {
			if (log.source && !seen.has(log.source)) {
				seen.add(log.source);
				sources.push(log.source);
			}
		}
		return sources;
	});

	const tabs = $derived([
		{ name: m.logs_filter_all(), value: "all" },
		...uniqueSources.map((source) => ({ name: sourceName(source), value: source })),
	]);

	const filteredLogs = $derived(
		activeSource === "all" ? $processLogs : (
			$processLogs.filter((log) => log.source === activeSource)
		),
	);

	$effect(() => {
		if (activeSource !== "all" && !uniqueSources.includes(activeSource)) {
			activeSource = "all";
		}
	});

	function sourceName(source: string): string {
		switch (source) {
			case "launch": {
				return m.logs_filter_launch();
			}
			case "lobby": {
				return m.logs_filter_lobby();
			}
			case "rigby": {
				return m.logs_filter_rigby();
			}
			case "rigby-queue": {
				return m.logs_filter_rigby_queue();
			}
			case "mods": {
				return m.logs_filter_mods();
			}
			default: {
				return source;
			}
		}
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header
		title={m.logs_title()}
		{tabs}
		activeTab={activeSource}
		onSelectTab={(source) => (activeSource = source)}
	>
		{#snippet icon()}
			<ScrollText size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
			<button
				class="btn btn-ghost"
				onclick={clearProcessLogs}
				disabled={$processLogs.length === 0}
			>
				<Trash2 size={16} />
				{m.common_clear()}
			</button>
		{/snippet}
	</Header>

	<div class="flex-1 overflow-hidden">
		{#key activeSource}
			<GhosttyTerminal logs={filteredLogs} />
		{/key}
	</div>
</div>
