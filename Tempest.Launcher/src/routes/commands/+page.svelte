<script lang="ts">
	import { Terminal, Search, X } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import { m } from "$lib/paraglide/messages";
	import { commandsPageOpen } from "$lib/stores/ui.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { categoryGroups, groupLabels, catLabels, catMap, allCommands, categoryKeys, sigMap, type Cmd } from "$lib/data/commands";

	sigMap.set("onrightmousepressed (climbing)", "OnRightMousePressed()");
	sigMap.set("onleftmousepressed (climbing)", "OnLeftMousePressed()");
	sigMap.set("onleftmousepressed (dead)", "OnLeftMousePressed()");
	sigMap.set("onrightmousepressed (dead)", "OnRightMousePressed()");
	sigMap.set("viewplayerbyname (spectating)", "ViewPlayerByName(string PlayerName)");
	sigMap.set("prevweapon (spectating)", "PrevWeapon()");
	sigMap.set("nextweapon (spectating)", "NextWeapon()");
	sigMap.set("onrightmousepressed (spectating)", "OnRightMousePressed()");

	let enabledKeys = $state(new Set(categoryKeys));
	let search = $state("");

	function toggleCategory(key: string) {
		if (enabledKeys.has(key)) {
			if (enabledKeys.size === 1) return;
			enabledKeys.delete(key);
		} else {
			enabledKeys.add(key);
		}
		enabledKeys = new Set(enabledKeys);
	}

	const visible = $derived(
		allCommands.filter(
			(cmd) =>
				enabledKeys.has(cmd.catKey) &&
				(search === "" || cmd.name.toLowerCase().includes(search.toLowerCase())),
		),
	);

	const count = $derived(visible.length);

	let expandedKey = $state<string | null>(null);
	let descriptions = $state<Record<string, string>>({});

	function getSig(name: string): string {
		return sigMap.get(name.toLowerCase()) ?? `${name}()`;
	}

	function toggleExpanded(key: string) {
		expandedKey = expandedKey === key ? null : key;
	}
</script>

<svelte:head>
	<title>Commands</title>
	<meta name="description" content="In-game console command reference" />
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	<Header title={m.commands_page_title()}>
		{#snippet icon()}
			<Terminal size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>{m.commands_subtitle_f2()}</span>
		{/snippet}
		{#snippet actions()}
			<label class="input input-bordered">
				<Search size={16} class="opacity-50" />
				<input
					type="text"
					placeholder="Search commands..."
					class="grow"
					bind:value={search}
				/>
				{#if count !== allCommands.length}
					<span class="text-xs text-base-content/50 shrink-0">{count}/{allCommands.length}</span>
				{/if}
			</label>
			<button
				class="btn btn-circle btn-ghost"
				onclick={() => { commandsPageOpen.value = false; goto("/"); }}
				aria-label={m.common_close()}
			>
				<X size={16} />
			</button>
		{/snippet}
	</Header>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				<div class="flex flex-col gap-3 mb-6">
					{#each categoryGroups as group}
						<div>
							<span class="text-xs font-semibold uppercase tracking-wider text-base-content/40 mb-1 block">
								{groupLabels[group.label]}
							</span>
							<div class="flex flex-wrap gap-x-3 gap-y-1">
								{#each group.categories as { name, color, key }}
									<label class="flex items-center gap-1.5 cursor-pointer select-none text-sm group">
										<input
											type="checkbox"
											checked={enabledKeys.has(key)}
											onchange={() => toggleCategory(key)}
											class="hidden"
										/>
										<span
											class="inline-flex items-center justify-center w-3.5 h-3.5 rounded transition-colors shrink-0"
											style={`
												background-color: ${enabledKeys.has(key) ? color : 'transparent'};
												border: 2px solid ${color};
											`}
										>
											{#if enabledKeys.has(key)}
												<svg class="w-2.5 h-2.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round">
													<polyline points="20 6 9 17 4 12" />
												</svg>
											{/if}
										</span>
										<span style="color: {color}">{catLabels[key]}</span>
									</label>
								{/each}
							</div>
						</div>
					{/each}
				</div>

				{#if visible.length > 0}
					<div class="flex flex-wrap gap-1.5 items-start">
						{#each visible as cmd (cmd.name + cmd.catKey)}
							{@const cat = catMap.get(cmd.catKey)!}
							{@const key = cmd.name + cmd.catKey}
							{@const expanded = expandedKey === key}
							<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
							<div
								class="cmd-badge cursor-pointer select-none"
								class:expanded
								role="button"
								tabindex="0"
								onselectstart={(e) => { if ((e.target as HTMLElement).tagName !== 'TEXTAREA') e.preventDefault(); }}
								onclick={(e) => {
									if ((e.target as HTMLElement).tagName === "TEXTAREA") return;
									toggleExpanded(key);
								}}
								onkeydown={(e) => {
									if ((e.target as HTMLElement).tagName === "TEXTAREA") return;
									if (e.key === 'Enter' || e.key === ' ') toggleExpanded(key);
								}}
								style={`
									--cat-color: ${cat.color};
									--cat-bg: color-mix(in srgb, ${cat.color} 18%, transparent);
									--cat-border: color-mix(in srgb, ${cat.color} 40%, transparent);
								`}
							>
								{#if expanded}
									<div class="flex flex-col gap-1.5">
										<div class="flex items-center gap-2">
											<span class="font-mono text-xs font-bold shrink-0 select-none">{cmd.name}</span>
											<span class="text-[10px] text-base-content/30 bg-base-300 px-1.5 py-0.5 rounded font-mono select-none">{catLabels[cat.key]}</span>
										</div>
										<div class="bg-base-300/50 rounded-lg p-2 overflow-x-auto">
											<code class="text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all select-none">{getSig(cmd.name)}</code>
										</div>
										<textarea
											class="w-full text-xs bg-base-300/50 border border-base-300 rounded px-2 py-1 resize-none outline-none focus:outline-none leading-tight"
											rows={1}
											placeholder="Add a description..."
											value={descriptions[key] ?? ""}
											oninput={(e) => {
												descriptions[key] = e.currentTarget.value;
											}}
										></textarea>
									</div>
								{:else}
									<span class="font-mono text-xs select-none">{cmd.name}</span>
								{/if}
							</div>
						{/each}
					</div>
				{:else}
					<div class="flex flex-col items-center justify-center h-48 gap-3 text-base-content/50">
						<Search size={40} class="opacity-30" />
						<p class="text-lg">No commands matching "{search}"</p>
					</div>
				{/if}

				<div class="mt-6 text-[11px] text-base-content/50 leading-relaxed">
					{m.commands_disclaimer()}
				</div>
			</div>
		</div>
	</div>
</div>


<style>
	.cmd-badge {
		user-select: none;
		display: inline-flex;
		align-items: center;
		border-radius: 0.5rem;
		border: 1px solid var(--cat-border);
		background-color: var(--cat-bg);
		color: var(--cat-color);
		transition: color 120ms, background-color 120ms, border-color 120ms, transform 120ms, padding 120ms, border-radius 120ms;
		padding: 0.25rem 0.625rem;
	}
	.cmd-badge:not(.expanded):hover {
		background-color: color-mix(in srgb, white 20%, transparent);
		border-color: white;
		color: white;
		transform: scale(1.05);
	}
	.cmd-badge:not(.expanded):active {
		transform: scale(0.95);
	}
	.cmd-badge.expanded {
		background-color: color-mix(in srgb, var(--cat-color) 6%, var(--fallback-b2, oklch(var(--b2))));
		border-color: var(--cat-border);
		padding: 0.5rem;
		border-radius: 0.5rem;
		animation: cmd-expand-in 150ms ease-out;
	}
	.cmd-badge.expanded textarea {
		color: white;
		min-height: 0;
	}
	.cmd-badge.expanded textarea::placeholder {
		color: rgba(255, 255, 255, 0.45);
	}
	.cmd-badge.expanded textarea:focus {
		outline: none !important;
		border-color: var(--cat-border);
	}

	@keyframes cmd-expand-in {
		from {
			opacity: 0;
			transform: scale(0.95);
		}
		to {
			opacity: 1;
			transform: scale(1);
		}
	}
</style>
