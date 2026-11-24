<script lang="ts">
	import { ScrollText, Search, Trash2, Copy, Check } from "@lucide/svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import TerminalView from "$lib/components/ui/TerminalView.svelte";
	import { m } from "$lib/paraglide/messages";
	import { clearProcessLogs, processLogs } from "$lib/stores/processes.svelte";
	import { ansiRuns, normalizeLine } from "$lib/utils/ansi";

	let activeSource = $state("all");
	let query = $state("");
	let copyStatus = $state<"idle" | "copied" | "failed">("idle");

	async function handleCopy() {
		const textToCopy = filteredLogs
			.map((log) =>
				activeSource === "all" && log.source ? `[${log.source}] ${log.line}` : log.line,
			)
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

	const sourceLogs = $derived(
		activeSource === "all"
			? processLogs.value
			: processLogs.value.filter((log) => log.source === activeSource),
	);
	const needle = $derived(query.trim().toLowerCase());
	// Against the text as shown: escape codes can split a word in the raw line.
	const filteredLogs = $derived(
		needle
			? sourceLogs.filter((log) =>
					ansiRuns(normalizeLine(log.line))
						.map((run) => run.text)
						.join("")
						.toLowerCase()
						.includes(needle),
				)
			: sourceLogs,
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
			case "rigby": {
				return m.logs_filter_rigby();
			}
			case "rigby-queue": {
				return m.logs_filter_rigby_queue();
			}
			case "mods": {
				return m.logs_filter_mods();
			}
			case "setup": {
				return m.logs_filter_setup();
			}
			default: {
				return source;
			}
		}
	}
</script>

<div class="flex h-full flex-col">
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
			<button class="btn btn-ghost" onclick={handleCopy} disabled={filteredLogs.length === 0}>
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

	<div class="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden p-4">
		<div
			data-panel
			class="bg-base-200 border-base-300 rounded-box min-h-0 flex-1 overflow-hidden border p-1"
		>
			<!-- The terminal only appends, so another set of lines needs a fresh one -->
			{#key `${activeSource}:${needle}`}
				<TerminalView logs={filteredLogs} showPrefix={activeSource === "all"} />
			{/key}
		</div>
		<label class="input input-sm w-full shrink-0">
			<Search size={14} class="opacity-50" />
			<input
				type="search"
				class="grow"
				placeholder={m.common_search()}
				aria-label={m.common_search()}
				bind:value={query}
			/>
			{#if needle}
				<span class="text-base-content/60 shrink-0 text-xs tabular-nums">
					{filteredLogs.length} / {sourceLogs.length}
				</span>
			{/if}
		</label>
	</div>
</div>
