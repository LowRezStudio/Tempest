<!--
	New mod project, in three steps: name the mod, pick the game folder, pick
	where the project lives. Creates the folder (the project file with every
	edit, mod-files/ for scripts and other files, builds/ for the packages),
	opens the game folder and keeps the project saved as you work.
-->
<script lang="ts">
	import { Check, FolderOpen, Hammer } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { invoke, pickDirectory } from "$lib/platform/api";
	import { MOD_ID_PATTERN, SEMVER_PATTERN, slugify } from "$lib/studio/cooker.svelte";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let step = $state(0);
	let name = $state("");
	let author = $state("");
	let version = $state("1.0.0");
	let gameDir = $state("");
	let parent = $state("");
	let busy = $state(false);
	let gameOk = $state<boolean | null>(null);

	const id = $derived(slugify(name));
	const dir = $derived(parent && id ? `${parent}/${id}` : "");
	const steps = ["Your mod", "Game folder", "Project folder"];

	$effect(() => {
		if (!open) return;
		step = 0;
		name = "";
		version = "1.0.0";
		author = localStorage.getItem("studio.author") ?? "";
		gameDir = studio.workspace?.dir ?? studio.recents[0] ?? "";
		void invoke("app:documents").then((d) => (parent = `${d}/Tempest Mods`));
	});

	$effect(() => {
		const d = gameDir;
		gameOk = null;
		if (d) void invoke("fs:exists", { path: `${d}/Core.upk` }).then((ok) => (gameOk = ok));
	});

	const valid = $derived([
		name.trim() !== "" && MOD_ID_PATTERN.test(id) && SEMVER_PATTERN.test(version.trim()),
		gameOk === true,
		dir !== "",
	]);

	async function browseGame() {
		const d = await pickDirectory("The game's CookedPCConsole folder", gameDir || undefined);
		if (d) gameDir = d;
	}

	async function browseParent() {
		const d = await pickDirectory("Where to keep your mod projects", parent || undefined);
		if (d) parent = d;
	}

	async function create() {
		busy = true;
		try {
			localStorage.setItem("studio.author", author.trim());
			if (await invoke("fs:exists", { path: `${dir}/${id}.twrk` })) {
				await session.open(`${dir}/${id}.twrk`);
				open = false;
				return;
			}
			const ok = await session.createProject({
				dir,
				name: name.trim(),
				id,
				author: author.trim(),
				version: version.trim(),
				gameDir,
			});
			if (ok) {
				open = false;
				studio.mode = "packages";
			}
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title="New mod project" width="max-w-xl">
	<div class="flex flex-col gap-4 text-sm">
		<ol class="flex items-center gap-2 text-xs">
			{#each steps as s, i (s)}
				<li class="flex items-center gap-1.5 {i === step ? 'font-semibold' : 'opacity-60'}">
					<span
						class="flex h-5 w-5 items-center justify-center rounded-full text-[11px] {i <
						step
							? 'bg-success text-success-content'
							: i === step
								? 'bg-primary text-primary-content'
								: 'bg-base-300'}"
						>{#if i < step}<Check size={11} />{:else}{i + 1}{/if}</span
					>
					{s}
				</li>
				{#if i < steps.length - 1}<li class="bg-base-300 h-px flex-1"></li>{/if}
			{/each}
		</ol>

		{#if step === 0}
			<p class="text-xs opacity-70">
				A project keeps everything for one mod: your edits, its files and its builds. It
				saves itself as you work, so you can close the Studio and come back to it later.
			</p>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Mod name</span>
				<!-- svelte-ignore a11y_autofocus -->
				<input
					class="input input-sm w-full"
					bind:value={name}
					placeholder="Kasumi Icon Tweak"
					autofocus
				/>
				{#if name.trim()}
					<span
						class="text-[11px] {MOD_ID_PATTERN.test(id) ? 'opacity-50' : 'text-error'}"
						>id: <code>{id || "?"}</code></span
					>
				{/if}
			</label>
			<div class="grid grid-cols-2 gap-3">
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Author</span>
					<input
						class="input input-sm w-full"
						bind:value={author}
						placeholder="your name"
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Version</span>
					<input
						class="input input-sm w-full font-mono {SEMVER_PATTERN.test(version.trim())
							? ''
							: 'input-error'}"
						bind:value={version}
					/>
				</label>
			</div>
		{:else if step === 1}
			<p class="text-xs opacity-70">
				The game folder your mod is made for: its <code>ChaosGame/CookedPCConsole</code> folder.
				Its files are the originals your changes are compared with.
			</p>
			<div class="flex gap-2">
				<input
					class="input input-sm min-w-0 flex-1 font-mono text-xs"
					bind:value={gameDir}
					placeholder="…/ChaosGame/CookedPCConsole"
				/>
				<button class="btn btn-sm" onclick={browseGame}
					><FolderOpen size={14} /> Browse…</button
				>
			</div>
			{#if gameDir && gameOk === false}
				<p class="text-error text-xs">
					This is not a CookedPCConsole folder (no Core.upk in it).
				</p>
			{/if}
			{#if studio.recents.length > 0}
				<div class="flex flex-col gap-1">
					<span class="text-xs opacity-60">Recent</span>
					{#each studio.recents.slice(0, 4) as r (r)}
						<button
							class="btn btn-ghost btn-xs justify-start truncate font-mono font-normal"
							onclick={() => (gameDir = r)}>{r}</button
						>
					{/each}
				</div>
			{/if}
		{:else}
			<p class="text-xs opacity-70">Where the project folder goes:</p>
			<div class="flex gap-2">
				<input
					class="input input-sm min-w-0 flex-1 font-mono text-xs"
					bind:value={parent}
				/>
				<button class="btn btn-sm" onclick={browseParent}
					><FolderOpen size={14} /> Browse…</button
				>
			</div>
			<div class="bg-base-200 rounded-lg p-3 font-mono text-xs leading-relaxed">
				<div class="truncate font-semibold" title={dir}>{dir}/</div>
				<div class="pl-4">
					<span class="opacity-90">{id}.twrk</span>
					<span class="opacity-50">· your edits and settings, saved as you work</span>
				</div>
				<div class="pl-4">
					<span class="opacity-90">mod-files/</span>
					<span class="opacity-50">· scripts, DLLs and other files to ship</span>
				</div>
				<div class="pl-4">
					<span class="opacity-90">builds/</span>
					<span class="opacity-50">· the .tempest packages you build</span>
				</div>
			</div>
		{/if}

		<div class="flex justify-between gap-2">
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => (step === 0 ? (open = false) : step--)}
			>
				{step === 0 ? "Cancel" : "Back"}
			</button>
			{#if step < 2}
				<button
					class="btn btn-primary btn-sm"
					disabled={!valid[step]}
					onclick={() => step++}>Next</button
				>
			{:else}
				<button
					class="btn btn-primary btn-sm"
					disabled={!valid[2] || busy}
					onclick={create}
				>
					{#if busy}<span class="loading loading-xs loading-spinner"></span>{:else}<Hammer
							size={14}
						/>{/if}
					Create project
				</button>
			{/if}
		</div>
	</div>
</Modal>
