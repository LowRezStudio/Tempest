<!--
	Settings: updates (version, check, download and restart, release notes,
	automatic check), appearance (theme, zoom), the warnings the user can turn
	back on, signing and publishing (shared with the mod cooker), automation
	(the MCP server) and the recent lists.
-->
<script lang="ts">
	import { Download, ExternalLink, RefreshCw } from "@lucide/svelte";
	import ModAccount from "$lib/components/ModAccount.svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { invoke } from "$lib/platform/api";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { remember, remembered, ui } from "$lib/studio/ui.svelte";
	import { updater } from "$lib/studio/updater.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	/** "Overwrite the game's files?" before Save into the game folder. */
	let warnInPlace = $state(true);

	$effect(() => {
		if (!open) return;
		warnInPlace = !remembered("save-in-place");
		if (updater.status === "idle" || updater.status === "error") void updater.check(true);
	});

	const u = $derived(updater.update);
	const statusText = $derived(
		{
			idle: "",
			checking: "Checking…",
			"up-to-date": "You have the newest version.",
			available: `Version ${u?.version} is available.`,
			downloading: `Downloading ${u?.version}… ${updater.percent}%`,
			ready: `${u?.version} is downloaded: restart to install it.`,
			installing: "Installing…",
			error: `Could not check: ${updater.error ?? "unknown error"}`,
		}[updater.status],
	);
</script>

<Modal bind:open title="Settings" width="max-w-2xl">
	<div class="flex flex-col gap-6 text-sm">
		<!-- Updates -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Updates</h3>
			<div class="flex flex-wrap items-center gap-2">
				<span class="opacity-70">Tempest Studio {u?.current ?? ""}</span>
				<span class="flex-1 text-xs opacity-70">{statusText}</span>
				<button
					class="btn btn-ghost btn-xs"
					disabled={updater.status === "checking" || updater.status === "downloading"}
					onclick={() => updater.check()}><RefreshCw size={12} /> Check now</button
				>
				{#if u?.available}
					{#if u.canInstall}
						{#if updater.status === "available"}
							<button
								class="btn btn-primary btn-xs"
								onclick={() => updater.download()}
								><Download size={12} /> Download</button
							>
						{:else if updater.status === "ready"}
							<button class="btn btn-primary btn-xs" onclick={() => updater.install()}
								>Restart to update</button
							>
						{/if}
					{:else if u.page}
						<button
							class="btn btn-primary btn-xs"
							onclick={() => u.page && invoke("shell:open-url", { url: u.page })}
							><ExternalLink size={12} /> Download from GitHub</button
						>
					{/if}
				{/if}
			</div>
			{#if updater.status === "downloading"}
				<progress class="progress progress-primary w-full" value={updater.percent} max="100"
				></progress>
			{/if}
			{#if u?.available && u.notes}
				<div
					class="bg-base-200 selectable max-h-48 overflow-y-auto rounded p-2 text-xs whitespace-pre-wrap"
				>
					{u.notes}
				</div>
			{/if}
			{#if u?.available && !u.canInstall}
				<p class="text-xs opacity-60">
					This install cannot update itself (macOS, or a Linux package without its package
					manager): download the new version from the release page.
				</p>
			{/if}
			<label class="flex items-center gap-2 text-xs">
				<input
					type="checkbox"
					class="checkbox checkbox-xs"
					checked={updater.auto}
					onchange={(e) => updater.setAuto(e.currentTarget.checked)}
				/>
				Check for a new version when the Studio starts
			</label>
		</section>

		<!-- Appearance -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Appearance</h3>
			<div class="flex flex-wrap items-center gap-4 text-xs">
				<div class="flex items-center gap-2">
					<span class="opacity-70">Theme</span>
					<div class="join">
						<button
							class="btn btn-xs join-item {studio.theme === 'mocha'
								? 'btn-active'
								: ''}"
							onclick={() => (studio.theme = "mocha")}>Dark</button
						>
						<button
							class="btn btn-xs join-item {studio.theme === 'latte'
								? 'btn-active'
								: ''}"
							onclick={() => (studio.theme = "latte")}>Light</button
						>
					</div>
				</div>
				<div class="flex items-center gap-2">
					<span class="opacity-70">Zoom</span>
					<div class="join">
						<button
							class="btn btn-xs join-item"
							onclick={() => ui.setZoom(ui.zoom - 0.1)}>−</button
						>
						<span class="btn btn-xs join-item pointer-events-none w-14 font-mono"
							>{Math.round(ui.zoom * 100)}%</span
						>
						<button
							class="btn btn-xs join-item"
							onclick={() => ui.setZoom(ui.zoom + 0.1)}>+</button
						>
					</div>
					<button class="btn btn-ghost btn-xs" onclick={() => ui.setZoom(1)}>Reset</button
					>
				</div>
			</div>
		</section>

		<!-- Warnings -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Warnings</h3>
			<label class="flex items-start gap-2 text-xs">
				<input
					type="checkbox"
					class="checkbox checkbox-xs mt-0.5"
					bind:checked={warnInPlace}
					onchange={() => remember("save-in-place", !warnInPlace)}
				/>
				<span
					>Warn before saving into the game folder
					<span class="block opacity-60"
						>Saving in place changes your game install; workspace files, projects and
						built mods leave it alone.</span
					></span
				>
			</label>
		</section>

		<!-- Signing and publishing -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Signing and publishing</h3>
			<ModAccount />
		</section>

		<!-- Automation -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Automation</h3>
			<div class="flex items-center gap-2 text-xs">
				<span class="flex-1 opacity-70"
					>AI assistants can drive the Studio through its MCP server.</span
				>
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => {
						open = false;
						ui.open("mcp");
					}}>MCP server…</button
				>
			</div>
		</section>

		<!-- Recent lists -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Recent lists</h3>
			<div class="flex flex-wrap gap-2 text-xs">
				<button
					class="btn btn-ghost btn-xs"
					disabled={session.recents.length === 0}
					onclick={() => session.forgetRecent()}
					>Clear recent projects ({session.recents.length})</button
				>
				<button
					class="btn btn-ghost btn-xs"
					disabled={studio.recents.length === 0}
					onclick={() => studio.forgetRecent()}
					>Clear recent game folders ({studio.recents.length})</button
				>
			</div>
			<p class="text-xs opacity-60">
				Only the lists are cleared: the files stay where they are.
			</p>
		</section>
	</div>
</Modal>
