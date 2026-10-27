<script lang="ts">
	import { AppWindow, Clock, FileUp, FolderOpen, PackageOpen } from "@lucide/svelte";
	import { onMount } from "svelte";
	import { invoke, pickDirectory, pickFile, PACKAGE_FILTERS } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { session } from "$lib/studio/session.svelte";
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

<div class="flex h-full items-center justify-center overflow-y-auto p-6">
	<div class="w-[min(36rem,100%)] space-y-6">
		<div class="flex items-center gap-3">
			<img src="/studio-icon.png" alt="" class="h-12 w-12" />
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
		<div class="grid grid-cols-2 gap-2">
			<button
				class="btn btn-sm"
				onclick={() => session.pickAndOpen()}
				disabled={studio.busy !== null}
				title="A .twrk saved from the Studio: unsaved changes, open package, tabs and layout"
			>
				<PackageOpen size={15} /> Open workspace file…
			</button>
			<button class="btn btn-sm" onclick={openFile} disabled={studio.busy !== null}>
				<FileUp size={15} /> Open a package file…
			</button>
		</div>
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

		{#if session.recents.length > 0}
			<div>
				<div
					class="mb-1 flex items-center gap-1 text-xs font-semibold uppercase opacity-60"
				>
					<PackageOpen size={12} /> Recent workspace files
				</div>
				<ul class="menu rounded-box bg-base-200 w-full overflow-hidden p-1">
					{#each session.recents as file (file)}
						<li>
							<button
								class="flex-col items-start gap-0"
								onclick={() => session.open(file)}
								disabled={studio.busy !== null}
								title={file}
							>
								<span class="font-semibold">{basename(file)}</span>
								<span class="max-w-full truncate font-mono text-[11px] opacity-60"
									>{file}</span
								>
							</button>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if studio.recents.length > 0}
			<div>
				<div
					class="mb-1 flex items-center gap-1 text-xs font-semibold uppercase opacity-60"
				>
					<Clock size={12} /> Recent game folders
				</div>
				<ul class="menu rounded-box bg-base-200 w-full overflow-hidden p-1">
					{#each studio.recents as dir (dir)}
						<li class="min-w-0 flex-row flex-nowrap items-center">
							<button
								class="min-w-0 flex-1 truncate font-mono text-xs"
								title={dir}
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
