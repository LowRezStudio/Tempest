<script lang="ts">
	import { ScrollText, Trash2, Copy, Check } from "@lucide/svelte";
	import GhosttyTerminal from "$lib/components/ui/GhosttyTerminal.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { clearProcessLogs, processLogs } from "$lib/stores/processes.svelte";

	let activeSource = $state("all");
	let copyStatus = $state<"idle" | "copied" | "failed">("idle");

	async function handleCopy() {
		const textToCopy = filteredLogs
			.map((log) => (activeSource === "all" && log.source ? `[${log.source}] ${log.line}` : log.line))
			.join("\n");
		try {
			await navigator.clipboard.writeText(textToCopy);
			copyStatus = "copied";
		} catch (error) {
			console.error("Failed to copy logs:", error);
			copyStatus = "failed";
		}
		setTimeout(() => {
			copyStatus = "idle";
		}, 2000);
	}

	const uniqueSources = $derived.by(() => {
		const seen = new Set<string>();
		const sources: string[] = [];
		for (const log of processLogs.value) {
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
		activeSource === "all" ? processLogs.value : (
			processLogs.value.filter((log) => log.source === activeSource)
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
				onclick={handleCopy}
				disabled={filteredLogs.length === 0}
			>
				{#if copyStatus === "copied"}
					<Check size={16} class="text-success" />
					{m.common_copied()}
				{:else if copyStatus === "failed"}
					{m.common_copy_failed()}
				{:else}
					<Copy size={16} />
					{m.common_copy_all()}
				{/if}
			</button>
			<button
				class="btn btn-ghost"
				onclick={clearProcessLogs}
				disabled={processLogs.value.length === 0}
			>
				<Trash2 size={16} />
				{m.common_clear()}
			</button>
		{/snippet}
	</Header>

	<div class="flex-1 overflow-hidden">
		{#key activeSource}
			<GhosttyTerminal logs={filteredLogs} showPrefix={activeSource === "all"} />
		{/key}
	</div>
</div>
