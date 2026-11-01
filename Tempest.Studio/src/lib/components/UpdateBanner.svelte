<!-- A newer Tempest Studio is published: a strip over the window with what to do about it. -->
<script lang="ts">
	import { Download, ExternalLink, RefreshCw, X } from "@lucide/svelte";
	import { invoke } from "$lib/platform/api";
	import { ui } from "$lib/studio/ui.svelte";
	import { updater } from "$lib/studio/updater.svelte";

	const u = $derived(updater.update);
	const shown = $derived(
		!updater.dismissed &&
			!!u?.available &&
			["available", "downloading", "ready", "installing"].includes(updater.status),
	);
</script>

{#if shown && u}
	<div
		class="bg-primary/15 border-primary/40 flex flex-wrap items-center gap-2 border-b px-3 py-1 text-xs"
	>
		<Download size={13} />
		<span class="flex-1">
			Tempest Studio {u.version} is out (you have {u.current}).
			{#if updater.status === "downloading"}Downloading… {updater.percent}%{/if}
			{#if updater.status === "ready"}Ready to install.{/if}
		</span>
		<button class="btn btn-ghost btn-xs" onclick={() => ui.open("settings")}>What's new</button>
		{#if u.canInstall}
			{#if updater.status === "available"}
				<button class="btn btn-primary btn-xs" onclick={() => updater.download()}
					><Download size={12} /> Download</button
				>
			{:else if updater.status === "ready" || updater.status === "installing"}
				<button
					class="btn btn-primary btn-xs"
					disabled={updater.status === "installing"}
					onclick={() => updater.install()}
					><RefreshCw size={12} /> Restart to update</button
				>
			{/if}
		{:else if u.page}
			<button
				class="btn btn-primary btn-xs"
				onclick={() => u.page && invoke("shell:open-url", { url: u.page })}
				><ExternalLink size={12} /> Download from GitHub</button
			>
		{/if}
		<button
			class="btn btn-ghost btn-xs btn-square"
			onclick={() => (updater.dismissed = true)}
			aria-label="Hide until next start"
			title="Hide until next start"><X size={12} /></button
		>
	</div>
{/if}
