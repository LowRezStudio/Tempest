<script lang="ts">
	import { Clock, FolderOpen, Package } from "@lucide/svelte";
	import { onMount } from "svelte";
	import { invoke, pickDirectory } from "$lib/platform/api";
	import { studio } from "$lib/studio/state.svelte";

	let backend = $state<{ running: boolean; binary: string | null; error: string | null } | null>(
		null,
	);

	onMount(async () => {
		backend = await invoke("upk:status");
	});

	async function browse() {
		const dir = await pickDirectory("Open a cooked folder (CookedPCConsole)");
		if (dir) await studio.openWorkspace(dir);
	}

	async function restart() {
		backend = await invoke("upk:restart");
	}
</script>

<div class="flex h-full items-center justify-center">
	<div class="w-[34rem] space-y-6">
		<div class="flex items-center gap-3">
			<Package size={36} class="text-primary" />
			<div>
				<h1 class="text-2xl font-bold">Tempest Studio</h1>
				<p class="text-sm opacity-70">Browse, extract and edit Paladins packages.</p>
			</div>
		</div>

		{#if backend && !backend.running}
			<div class="alert alert-error text-sm">
				<div>
					<div class="font-semibold">The upk backend is not running</div>
					<div class="selectable">{backend.error ?? "unknown error"}</div>
					<div class="mt-1 opacity-80">
						Build it with <code>cargo build --release</code> in Tempest.UnrealScript, then
						retry.
					</div>
				</div>
				<button class="btn btn-sm" onclick={restart}>Retry</button>
			</div>
		{/if}

		<button class="btn btn-primary w-full" onclick={browse} disabled={studio.busy !== null}>
			<FolderOpen size={18} />
			Open cooked folder…
		</button>
		<p class="text-xs opacity-60">
			Pick the game's <code>ChaosGame/CookedPCConsole</code> folder (or any folder holding the
			script packages). The class database is loaded from it; textures stream from its
			<code>.tfc</code> caches.
		</p>

		{#if studio.recents.length > 0}
			<div>
				<div
					class="mb-1 flex items-center gap-1 text-xs font-semibold uppercase opacity-60"
				>
					<Clock size={12} /> Recent
				</div>
				<ul class="menu rounded-box bg-base-200 w-full p-1">
					{#each studio.recents as dir (dir)}
						<li>
							<button
								class="font-mono text-xs"
								onclick={() => studio.openWorkspace(dir)}
								disabled={studio.busy !== null}
							>
								{dir}
							</button>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if studio.busy}
			<div class="flex items-center gap-2 text-sm">
				<span class="loading loading-sm loading-spinner"></span>
				{studio.busy}
			</div>
		{/if}
	</div>
</div>
