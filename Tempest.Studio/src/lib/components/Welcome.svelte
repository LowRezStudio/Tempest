<script lang="ts">
	import { AppWindow, Clock, FolderOpen, Package } from "@lucide/svelte";
	import { onMount } from "svelte";
	import { invoke, pickDirectory, pickFile, PACKAGE_FILTERS } from "$lib/platform/api";
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

	async function openFile() {
		const f = await pickFile("Open a package", PACKAGE_FILTERS);
		if (f) await studio.openFiles([f]);
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
		<button
			class="btn btn-ghost btn-sm w-full"
			onclick={openFile}
			disabled={studio.busy !== null}
		>
			Open a single package file…
		</button>
		{#if studio.pendingFiles.length > 0}
			<div class="alert alert-info text-xs">
				<span
					>{studio.pendingFiles.map((f) => f.split(/[/\\]/).at(-1)).join(", ")} will open once
					a game folder is chosen (it provides the class database).</span
				>
			</div>
		{/if}
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
						<li class="flex-row items-center">
							<button
								class="flex-1 font-mono text-xs"
								onclick={() => studio.openWorkspace(dir)}
								disabled={studio.busy !== null}
							>
								{dir}
							</button>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => studio.newWindow(dir)}
								title="Open in a new window"
							>
								<AppWindow size={12} />
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
