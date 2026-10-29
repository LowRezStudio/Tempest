<!--
	Build the mod: what goes in (one line per file, with its own action menu),
	the details, and the Build button. Game files you changed carry only your
	changes; the project's mod-files/ folder and added files go in as they are.
-->
<script lang="ts">
	import {
		ChevronDown,
		ChevronRight,
		Download,
		Ellipsis,
		FileBox,
		FileCode2,
		FileUp,
		FolderOpen,
		FolderPlus,
		Hammer,
		ImagePlus,
		KeyRound,
		Plus,
		RefreshCw,
		Send,
		X,
	} from "@lucide/svelte";
	import Menu, { type MenuItem } from "$lib/components/shell/Menu.svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker, type Entry } from "$lib/studio/cooker.svelte";
	import { basename, samePath } from "$lib/studio/format";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { CookSource } from "../../../electron/src/ipc/contract";

	let keyId = $state("");
	let apiKey = $state("");
	let open = $state<Record<string, boolean>>({});
	let more = $state(false);

	$effect(() => {
		if (studio.workspace) void cooker.refresh();
	});

	const m = $derived(cooker.meta);

	function sourceName(s: CookSource): string {
		if (s.origin === "unsaved") return session.project ? "your edits" : "your unsaved edits";
		if (s.origin === "saved") return "the file you saved in the game folder";
		const date = s.when ? new Date(s.when * 1000).toLocaleDateString() : "";
		return `the copy ${basename(s.source)}${date ? ` (${date})` : ""}`;
	}

	/** "3 changes", "no changes", "whole file", "…". */
	function changes(e: Entry): string {
		if (!e.choice.on) return "left out";
		if (!e.chosen) return "";
		if (e.choice.mode === "whole") return "whole file";
		const p = cooker.previews[e.chosen.id];
		if (!p) return "…";
		if (p.error) return "can't compare";
		const n = (p.changed ?? 0) + (p.added ?? 0) + (p.deleted ?? 0);
		return n === 0 ? "no changes" : `${n} change${n === 1 ? "" : "s"}`;
	}

	function actions(e: Entry): MenuItem[] {
		const items: MenuItem[] = [];
		const p = e.chosen ? cooker.previews[e.chosen.id] : undefined;
		if (p?.changes?.length) {
			items.push({
				label: open[e.target] ? "Hide the changes" : "Show the changes",
				run: () => (open = { ...open, [e.target]: !open[e.target] }),
			});
		}
		items.push({
			label: e.choice.on ? "Leave it out of the mod" : "Put it back in the mod",
			run: () => cooker.setChoice(e.target, { on: !e.choice.on }),
		});
		if (e.kind !== "pck" && !e.sources.some((s) => s.script)) {
			items.push({
				label: e.choice.mode === "whole" ? "Ship only my changes" : "Ship the whole file",
				detail:
					e.choice.mode === "whole"
						? "Recommended: works together with other mods"
						: "Replaces the game file; conflicts with other mods changing it",
				run: () =>
					cooker.setChoice(e.target, {
						mode: e.choice.mode === "whole" ? "delta" : "whole",
					}),
			});
		}
		if (e.sources.some((s) => s.origin === "unsaved")) {
			items.push("-", {
				label: "Discard my edits to this file…",
				detail: "Reloads it from the game folder (asks first)",
				run: () => cooker.discardEdits(e),
			});
		}
		if (!e.sources.some((s) => s.origin === "unsaved")) {
			items.push("-", {
				label: cooker.projectDir ? "Remove from the project" : "Remove from the list",
				detail: e.sources.some((s) => s.origin === "saved")
					? "Nothing is deleted; it comes back if you save the file again"
					: "This file is no longer part of the mod; nothing is deleted",
				run: () => cooker.dismiss(e),
			});
		}
		for (const s of e.sources.filter((x) => x.origin === "copy")) {
			items.push({
				label: `Forget the copy ${basename(s.source)}`,
				detail: "Removes it from this list; the file stays where it is",
				run: () => cooker.forgetCopy(s),
			});
		}
		return items;
	}
</script>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex max-w-3xl flex-col gap-6 p-4 text-sm">
		<header class="flex items-center gap-3">
			<Hammer size={22} class="text-primary shrink-0" />
			<div class="min-w-0 flex-1">
				<h2 class="truncate text-lg font-semibold">
					{m.name.trim() ? `Build ${m.name.trim()}` : "Build a mod"}
				</h2>
				<p class="truncate text-xs opacity-60">
					{session.project
						? `Builds go to ${session.project.dir}/builds`
						: "Tip: a mod project (New mod project in the menu) keeps all this for you."}
				</p>
			</div>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => cooker.refresh()}
				title="Look for new changes and files"
			>
				<RefreshCw size={14} /><span class="max-sm:hidden">Refresh</span>
			</button>
		</header>

		<!-- What goes in -->
		<section class="flex flex-col gap-2">
			<div class="flex items-center gap-2">
				<h3 class="font-semibold">What goes in</h3>
				<span class="flex-1"></span>
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => cooker.addFiles()}
					title="Packages and .dat files you modified elsewhere, Lua scripts, DLLs or any other file"
				>
					<FileUp size={12} /> Add files…
				</button>
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => cooker.addFolder()}
					title="A whole folder"
				>
					<FolderPlus size={12} /> Add folder…
				</button>
			</div>
			<div class="border-base-300 divide-base-300 divide-y rounded-lg border">
				{#each cooker.shown as e (e.target)}
					{@const p = e.chosen ? cooker.previews[e.chosen.id] : undefined}
					<div
						class="px-3 py-2"
						role="group"
						oncontextmenu={(ev) => ui.openContextMenu(ev, actions(e))}
					>
						<!-- A file left out is dimmed, but not its menu: a dropdown inside a
							translucent row would be translucent too. -->
						<div class="flex items-center gap-2">
							<FileBox
								size={15}
								class="shrink-0 {e.choice.on ? 'opacity-60' : 'opacity-30'}"
							/>
							<div class="min-w-0 flex-1 {e.choice.on ? '' : 'opacity-50'}">
								<div class="truncate font-mono text-xs" title={e.target}>
									{basename(e.target)}
								</div>
								{#if e.choice.on && e.chosen && (e.sources.length > 1 || e.chosen.origin !== "unsaved")}
									<div class="truncate text-[11px] opacity-60">
										from {sourceName(e.chosen)}
									</div>
								{/if}
							</div>
							<button
								class="btn btn-ghost btn-xs shrink-0 font-normal {e.choice.on
									? ''
									: 'opacity-50'}"
								disabled={!p?.changes?.length}
								onclick={() => (open = { ...open, [e.target]: !open[e.target] })}
								title="What the mod changes in this file"
							>
								{changes(e)}
								{#if p?.changes?.length && e.choice.on}
									{#if open[e.target]}<ChevronDown
											size={11}
										/>{:else}<ChevronRight size={11} />{/if}
								{/if}
							</button>
							<Menu
								items={actions(e)}
								buttonClass="btn btn-ghost btn-xs btn-square"
								title="Actions"
								align="end"
								width="w-80"
							>
								<Ellipsis size={14} />
							</Menu>
						</div>
						{#if e.choice.on && e.sources.length > 1}
							<div class="mt-1 ml-6 flex items-center gap-2 text-xs">
								<span class="opacity-60">Take the changes from</span>
								<select
									class="select select-xs min-w-0 flex-1"
									value={e.chosen?.id}
									onchange={(ev) =>
										cooker.setChoice(e.target, {
											source: ev.currentTarget.value,
										})}
								>
									{#each e.sources as src (src.id)}
										<option value={src.id}>{sourceName(src)}</option>
									{/each}
								</select>
							</div>
						{/if}
						{#if open[e.target] && e.choice.on}
							{#if p?.error}
								<p class="text-error mt-1 ml-6 text-xs">{p.error}</p>
							{:else if p?.changes?.length}
								<ul
									class="bg-base-200 mt-1 ml-6 max-h-40 overflow-y-auto rounded p-2 font-mono text-[11px]"
								>
									{#each p.changes as c, i (i)}
										<li class="truncate">
											<span
												class={c.op === "add"
													? "text-success"
													: c.op === "delete"
														? "text-error"
														: "opacity-60"}
												>{c.op === "set"
													? "changed"
													: c.op === "add"
														? "added"
														: "removed"}</span
											>
											{c.what}
										</li>
									{/each}
								</ul>
							{/if}
						{/if}
					</div>
				{/each}
				{#if session.project}
					<div class="flex items-center gap-2 px-3 py-2">
						<FolderOpen size={15} class="shrink-0 opacity-60" />
						<div class="min-w-0 flex-1">
							<div class="font-mono text-xs">mod-files/</div>
							<div class="text-[11px] opacity-60">
								{cooker.projectFiles.length === 0
									? "empty: put scripts (scripts/), DLLs (dlls/64/) or other files here"
									: `${cooker.projectFiles.length} file${cooker.projectFiles.length === 1 ? "" : "s"}, shipped as laid out`}
							</div>
						</div>
						<button
							class="btn btn-ghost btn-xs"
							onclick={() =>
								session.project &&
								invoke("shell:open-path", {
									path: `${session.project.dir}/mod-files`,
								})}>Open folder</button
						>
					</div>
				{/if}
				{#each cooker.extras as x, i (i)}
					<div class="flex items-center gap-2 px-3 py-2">
						<FileCode2 size={15} class="shrink-0 opacity-60" />
						<div class="min-w-0 flex-1">
							<div class="truncate font-mono text-xs" title={x.src}>
								{basename(x.src)}
							</div>
							<label class="flex items-center gap-1 text-[11px] opacity-70">
								goes to
								<input
									class="input input-xs h-5 min-w-0 flex-1 font-mono"
									bind:value={x.dest}
									title="Where it goes inside the mod: scripts/… for Lua, dlls/64/… for DLLs, files/ChaosGame/… for game files"
								/>
							</label>
						</div>
						<button
							class="btn btn-ghost btn-xs btn-square"
							onclick={() =>
								(cooker.extras = cooker.extras.filter((_, k) => k !== i))}
							title="Remove from the mod"
							aria-label="Remove {basename(x.src)}"><X size={13} /></button
						>
					</div>
				{/each}
				{#if cooker.shown.length === 0 && cooker.extras.length === 0 && !session.project}
					<p class="px-3 py-6 text-center text-xs opacity-60">
						Nothing yet. Edit a package or the assembly, or add files.
					</p>
				{/if}
			</div>
			<p class="text-xs opacity-60">
				Game files you changed carry only your changes, so your mod works with other mods.
				{#if cooker.unchanged.length > 0}
					{cooker.unchanged.length} file{cooker.unchanged.length === 1 ? "" : "s"} saved earlier
					no longer differ{cooker.unchanged.length === 1 ? "s" : ""} from the game and {cooker
						.unchanged.length === 1
						? "is"
						: "are"} left out.
				{/if}
			</p>
		</section>

		<!-- Details -->
		<section class="flex flex-col gap-3">
			<h3 class="font-semibold">Details</h3>
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-[2fr_1fr]">
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Name</span>
					<input
						class="input input-sm w-full"
						bind:value={cooker.meta.name}
						placeholder="My Champion"
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Version</span>
					<input
						class="input input-sm w-full font-mono {m.version && !cooker.versionValid
							? 'input-error'
							: ''}"
						bind:value={cooker.meta.version}
						placeholder="1.0.0"
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Author</span>
					<input
						class="input input-sm w-full"
						bind:value={cooker.meta.authors[0]!.name}
						placeholder="your name"
					/>
				</label>
				<div class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Icon</span>
					<div class="flex min-w-0 items-center gap-1">
						<button class="btn btn-sm" onclick={() => cooker.pickIcon()}
							><ImagePlus size={14} /> {m.icon ? "Change" : "Choose…"}</button
						>
						{#if m.icon}
							<span class="truncate font-mono text-xs" title={m.icon}
								>{basename(m.icon)}</span
							>
							<button
								class="btn btn-ghost btn-xs btn-square"
								onclick={() => (cooker.meta.icon = null)}
								aria-label="Remove the icon"><X size={12} /></button
							>
						{/if}
					</div>
				</div>
				<label class="flex flex-col gap-1 sm:col-span-2">
					<span class="text-xs opacity-70">Description</span>
					<textarea
						class="textarea textarea-sm w-full"
						rows="2"
						bind:value={cooker.meta.readme}
						placeholder="What the mod does, how to use it"></textarea>
				</label>
			</div>
			{#if m.version && !cooker.versionValid}
				<p class="text-error text-xs">
					The version must look like 1.0.0 (major.minor.patch, optionally 1.2.0-beta.1).
				</p>
			{/if}
			<button class="btn btn-ghost btn-xs w-fit" onclick={() => (more = !more)}>
				{#if more}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if} More options
			</button>
			{#if more}
				<div class="border-base-300 flex flex-col gap-3 rounded-lg border p-3">
					<label class="flex flex-col gap-1">
						<span class="text-xs opacity-70"
							>Mod id (made from the name unless you type one)</span
						>
						<input
							class="input input-sm w-full font-mono {!cooker.idValid && m.name.trim()
								? 'input-error'
								: ''}"
							value={cooker.modId}
							oninput={(ev) => {
								cooker.meta.idCustom = ev.currentTarget.value.trim() !== "";
								cooker.meta.id = ev.currentTarget.value.trim();
							}}
						/>
					</label>
					<div class="flex flex-col gap-1">
						<span class="flex items-center text-xs opacity-70">
							Authors and links
							<span class="flex-1"></span>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() =>
									(cooker.meta.authors = [
										...cooker.meta.authors,
										{ name: "", link: "" },
									])}><Plus size={12} /> Add</button
							>
						</span>
						{#each cooker.meta.authors as a, i (i)}
							<div class="flex gap-1">
								<input
									class="input input-xs w-32"
									placeholder="name"
									bind:value={a.name}
								/>
								<input
									class="input input-xs min-w-0 flex-1"
									placeholder="link (optional)"
									bind:value={a.link}
								/>
								{#if i > 0}
									<button
										class="btn btn-ghost btn-xs btn-square"
										onclick={() =>
											(cooker.meta.authors = cooker.meta.authors.filter(
												(_, k) => k !== i,
											))}
										aria-label="Remove author"><X size={12} /></button
									>
								{/if}
							</div>
						{/each}
					</div>
					<label class="flex flex-col gap-1">
						<span class="text-xs opacity-70"
							>Needs other mods (one per line, e.g. some-mod&gt;=1.0.0). The Lua
							runtime is added for you when the mod has scripts.</span
						>
						<textarea
							class="textarea textarea-sm w-full font-mono"
							rows="2"
							bind:value={cooker.meta.dependencies}></textarea>
					</label>
				</div>
			{/if}
		</section>

		<!-- Build -->
		<section class="bg-base-200 border-base-300 flex flex-col gap-3 rounded-lg border p-3">
			{#if cooker.cli && !cooker.cli.hasKey && cooker.cli.available}
				<div class="flex flex-wrap items-center gap-2 text-xs">
					<KeyRound size={14} />
					<span class="flex-1"
						>Mods with scripts or DLLs must be signed. Create your signing key once:</span
					>
					<input
						class="input input-xs w-44"
						placeholder="your name or email"
						bind:value={keyId}
					/>
					<button
						class="btn btn-xs"
						onclick={() => cooker.createKey(keyId)}
						disabled={!!cooker.busy}>Create key</button
					>
				</div>
			{/if}
			{#if cooker.problems.length > 0}
				<ul class="list-inside list-disc text-xs opacity-80">
					{#each cooker.problems as p, i (i)}<li>{p}</li>{/each}
				</ul>
			{/if}
			<div class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Compression</span>
				<div class="join w-fit" role="radiogroup" aria-label="Compression">
					<button
						class="btn btn-sm join-item {m.compression === 'small' ? 'btn-active' : ''}"
						role="radio"
						aria-checked={m.compression === "small"}
						onclick={() => (m.compression = "small")}
						title="LZMA: the smallest mod, slower to build (a big mod takes a minute or so)"
						>Smallest file</button
					>
					<button
						class="btn btn-sm join-item {m.compression === 'fast' ? 'btn-active' : ''}"
						role="radio"
						aria-checked={m.compression === "fast"}
						onclick={() => (m.compression = "fast")}
						title="zstd: builds several times faster, the mod is about 30 % bigger"
						>Faster build</button
					>
				</div>
				<span class="text-[11px] opacity-60"
					>{m.compression === "fast"
						? "Builds several times faster; the mod is about 30 % bigger. Good while testing."
						: "The smallest mod to download; slower to build. Best for publishing."}</span
				>
			</div>
			<div class="flex flex-wrap items-center gap-2">
				<button
					class="btn btn-primary"
					onclick={() => cooker.cook()}
					disabled={!cooker.canCook || !!cooker.busy}
				>
					{#if cooker.busy === "Cooking…"}<span class="loading loading-sm loading-spinner"
						></span>{:else}<Hammer size={16} />{/if}
					Build {cooker.versionValid ? m.version.trim() : ""}
				</button>
				{#if cooker.cooked}
					<button
						class="btn"
						onclick={() => cooker.install()}
						disabled={!!cooker.busy}
						title="Install it into this game folder to try it"
					>
						<Download size={16} /> Install to try it
					</button>
					<button
						class="btn btn-ghost btn-sm"
						onclick={() => cooker.install(true)}
						disabled={!!cooker.busy}>Install elsewhere…</button
					>
				{/if}
			</div>
			{#if cooker.cooked}
				<p class="text-xs">
					Built <button
						class="link font-mono"
						onclick={() =>
							cooker.cooked &&
							invoke("shell:show-item", { path: cooker.cooked.path })}
						>{basename(cooker.cooked.path)}</button
					>{cooker.cooked.signer ? `, signed by ${cooker.cooked.signer}` : ", unsigned"}.
				</p>
				<details class="border-base-300 rounded-lg border p-2">
					<summary class="cursor-pointer text-xs font-semibold"
						><Send size={12} class="inline" /> Publish to mods.lowrezstudio.com</summary
					>
					<div class="mt-2 grid gap-2 sm:grid-cols-2">
						<input
							class="input input-sm"
							placeholder="works from patch (e.g. 8.1)"
							bind:value={cooker.meta.patchFrom}
						/>
						<input
							class="input input-sm"
							placeholder="up to patch (empty: and later)"
							bind:value={cooker.meta.patchTo}
						/>
						<input
							class="input input-sm sm:col-span-2"
							placeholder="one-line summary (needed for a new mod)"
							bind:value={cooker.meta.summary}
						/>
						<input
							class="input input-sm"
							placeholder="tags, comma-separated"
							bind:value={cooker.meta.tags}
						/>
						<input
							class="input input-sm"
							placeholder="what changed in this version"
							bind:value={cooker.meta.changelog}
						/>
						<div class="flex gap-1 sm:col-span-2">
							<input
								class="input input-sm min-w-0 flex-1 font-mono"
								type="password"
								placeholder="API key (tm_live_…), once per machine"
								bind:value={apiKey}
							/>
							<button
								class="btn btn-sm"
								disabled={!apiKey.trim()}
								onclick={async () => {
									if (await cooker.login(apiKey)) apiKey = "";
								}}>Log in</button
							>
							<button
								class="btn btn-primary btn-sm"
								onclick={() => cooker.publish()}
								disabled={!!cooker.busy || !cooker.cooked.signer}
								><Send size={14} /> Publish</button
							>
						</div>
					</div>
				</details>
			{/if}
			{#if cooker.log.length > 0}
				<details>
					<summary class="cursor-pointer text-xs opacity-60">Build log</summary>
					<div
						class="bg-base-300 selectable mt-1 max-h-56 overflow-auto rounded p-2 font-mono text-[11px] whitespace-pre-wrap"
					>
						{#each cooker.log as l, i (i)}
							<div
								class={l.kind === "error"
									? "text-error"
									: l.kind === "ok"
										? ""
										: "opacity-60"}
							>
								{l.text}
							</div>
						{/each}
					</div>
				</details>
			{/if}
		</section>
	</div>
</div>
