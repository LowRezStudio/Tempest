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
		Plus,
		RefreshCw,
		Send,
		X,
	} from "@lucide/svelte";
	import ModAccount from "$lib/components/ModAccount.svelte";
	import PublishDialog from "$lib/components/PublishDialog.svelte";
	import Menu, { type MenuItem } from "$lib/components/shell/Menu.svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker, type Entry } from "$lib/studio/cooker.svelte";
	import { basename, samePath } from "$lib/studio/format";
	import { MAX_SUMMARY } from "$lib/studio/modmeta";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { CookSource } from "../../../electron/src/ipc/contract";

	let open = $state<Record<string, boolean>>({});
	let publishOpen = $state(false);
	let iconUrl = $state<string | null>(null);

	$effect(() => {
		if (studio.workspace) void cooker.refresh();
	});

	const m = $derived(cooker.meta);
	// Typed ids that cannot be one (an empty name is reported by Build instead).
	const idError = $derived(!cooker.idValid && cooker.modId !== "");

	// The icon's preview, from its bytes (checked on the way).
	$effect(() => {
		void m.icon;
		let url: string | null = null;
		void cooker.iconBytes().then((data) => {
			url = data ? URL.createObjectURL(new Blob([data as Uint8Array<ArrayBuffer>])) : null;
			iconUrl = url;
		});
		return () => {
			if (url) URL.revokeObjectURL(url);
		};
	});

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
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1 sm:col-span-2">
					<span class="text-xs opacity-70">Name</span>
					<input
						class="input input-sm w-full"
						bind:value={cooker.meta.name}
						placeholder="My Champion"
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Mod id</span>
					<input
						class="input input-sm w-full font-mono {idError ? 'input-error' : ''}"
						value={cooker.modId}
						spellcheck="false"
						placeholder="my-champion"
						aria-invalid={idError}
						oninput={(ev) => {
							cooker.meta.idCustom = ev.currentTarget.value.trim() !== "";
							cooker.meta.id = ev.currentTarget.value.trim();
						}}
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Version</span>
					<input
						class="input input-sm w-full font-mono {cooker.versionValid
							? ''
							: 'input-error'}"
						bind:value={cooker.meta.version}
						spellcheck="false"
						placeholder="1.0.0"
						aria-invalid={!cooker.versionValid}
					/>
				</label>
				{#if idError}
					<p class="text-error text-xs sm:col-span-2">
						“{cooker.modId}” cannot be a mod id: use 2 to 64 lowercase letters, digits,
						dots, dashes or underscores, starting with a letter or digit.
					</p>
				{:else}
					<p class="-mt-1 text-xs opacity-60 sm:col-span-2">
						The id names the mod on the repository and in other mods' dependencies; it
						follows the name unless you type one, and should never change once
						published.
					</p>
				{/if}
				{#if !cooker.versionValid}
					<p class="text-error flex flex-wrap items-center gap-2 text-xs sm:col-span-2">
						“{m.version.trim()}” is not a version: use major.minor.patch, like 1.0.0 or
						1.2.0-beta.1.
						{#if cooker.versionSuggestion}
							<button
								class="btn btn-xs"
								onclick={() =>
									(cooker.meta.version = cooker.versionSuggestion ?? "")}
								>Use {cooker.versionSuggestion}</button
							>
						{/if}
					</p>
				{/if}
				<div class="flex flex-col gap-1 sm:col-span-2">
					<span class="flex items-center text-xs opacity-70">
						Authors (with a link to their page, optional)
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
								class="input input-sm min-w-0 flex-1"
								placeholder="name"
								bind:value={a.name}
							/>
							<input
								class="input input-sm min-w-0 flex-1"
								placeholder="https://…"
								bind:value={a.link}
							/>
							<button
								class="btn btn-ghost btn-sm btn-square"
								disabled={cooker.meta.authors.length === 1}
								onclick={() =>
									(cooker.meta.authors = cooker.meta.authors.filter(
										(_, k) => k !== i,
									))}
								aria-label="Remove author"><X size={13} /></button
							>
						</div>
					{/each}
				</div>
			</div>
		</section>

		<!-- Listing -->
		<section class="flex flex-col gap-3">
			<h3 class="font-semibold">
				Listing <span class="text-xs font-normal opacity-50">· for the mod repository</span>
			</h3>
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1 sm:col-span-2">
					<span class="flex justify-between text-xs opacity-70"
						><span>Summary (one line, shown in the list)</span><span
							class={m.summary.length > MAX_SUMMARY ? "text-error" : ""}
							>{m.summary.length}/{MAX_SUMMARY}</span
						></span
					>
					<input
						class="input input-sm w-full"
						maxlength={MAX_SUMMARY}
						placeholder="What the mod does, in a sentence"
						bind:value={cooker.meta.summary}
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Tags</span>
					<input
						class="input input-sm w-full"
						placeholder="skin, maeve, cosmetic"
						bind:value={cooker.meta.tags}
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Source code (optional link)</span>
					<input
						class="input input-sm w-full"
						placeholder="https://github.com/…"
						bind:value={cooker.meta.repository}
					/>
				</label>
				<label class="flex flex-col gap-1 sm:col-span-2">
					<span class="text-xs opacity-70">Needs other mods</span>
					<textarea
						class="textarea textarea-sm w-full font-mono {cooker.dependencyProblems
							.length
							? 'textarea-error'
							: ''}"
						rows="2"
						placeholder={"some-mod>=1.0.0\ntempest>=1.4.3"}
						bind:value={cooker.meta.dependencies}></textarea>
					{#each cooker.dependencyProblems as d, i (i)}
						<span class="text-error text-xs">{d}</span>
					{/each}
					<span class="text-xs opacity-60"
						>One per line: a mod id, then optionally a version requirement (&gt;=1.0.0,
						^2, ~1.2, &gt;=1.0 &lt;2.0). <code>tempest</code> is the launcher. The Lua
						runtime ({cooker.hasCode
							? "added: the mod has scripts"
							: "added when the mod has scripts"}) needs no line.</span
					>
				</label>
			</div>
		</section>

		<!-- Icon and readme -->
		<section class="flex flex-col gap-3">
			<h3 class="font-semibold">Icon and readme</h3>
			<div class="grid gap-3 sm:grid-cols-[8rem_minmax(0,1fr)]">
				<div class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Icon</span>
					{#if iconUrl}
						<div
							class="bg-base-300 relative aspect-square w-32 overflow-hidden rounded-lg"
						>
							<img
								src={iconUrl}
								alt="Mod icon"
								title={m.icon ?? undefined}
								class="h-full w-full object-cover"
							/>
							<button
								class="btn btn-circle btn-xs absolute top-1 right-1"
								aria-label="Remove the icon"
								onclick={() => (cooker.meta.icon = null)}><X size={12} /></button
							>
						</div>
					{:else}
						<button
							class="border-base-300 hover:border-primary/60 flex aspect-square w-32 flex-col items-center justify-center gap-1 rounded-lg border-2 border-dashed text-xs opacity-70"
							onclick={() => cooker.pickIcon()}
							><ImagePlus size={18} /> Choose a PNG</button
						>
					{/if}
					{#if cooker.iconError}
						<span class="text-error text-xs">{cooker.iconError}</span>
					{:else}
						<span class="text-[11px] opacity-60">Square PNG, up to 1 MB.</span>
					{/if}
				</div>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70"
						>Readme (Markdown: what it does, how to use it)</span
					>
					<textarea
						class="textarea textarea-sm min-h-40 w-full flex-1"
						bind:value={cooker.meta.readme}
						placeholder={"# My mod\n\nWhat it changes, what it needs, known issues…"}
					></textarea>
				</label>
			</div>
		</section>

		<!-- Build -->
		<section class="bg-base-200 border-base-300 flex flex-col gap-3 rounded-lg border p-3">
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
			</div>
			{#if cooker.cooked}
				<div
					class="border-success/40 bg-success/10 flex flex-col gap-2 rounded-lg border p-3"
				>
					<p class="text-xs">
						Built <button
							class="link font-mono"
							onclick={() =>
								cooker.cooked &&
								invoke("shell:show-item", { path: cooker.cooked.path })}
							>{basename(cooker.cooked.path)}</button
						>{cooker.cooked.signer
							? `, signed by ${cooker.cooked.signer}`
							: ", unsigned (assets only: mods with code must be signed)"}.
					</p>
					<div class="flex flex-wrap gap-2">
						<button
							class="btn btn-primary btn-sm"
							onclick={() => (publishOpen = true)}
							disabled={!!cooker.busy}
							title={cooker.cooked.signer
								? "Upload it to the mod repository"
								: "Only signed mods can be published"}
							><Send size={14} /> Publish…</button
						>
						<button
							class="btn btn-sm"
							onclick={() => cooker.install()}
							disabled={!!cooker.busy}
							title="Install it into this game folder to try it"
						>
							<Download size={14} /> Install to try it
						</button>
						<button
							class="btn btn-ghost btn-sm"
							onclick={() => cooker.install(true)}
							disabled={!!cooker.busy}>Install elsewhere…</button
						>
						<button
							class="btn btn-ghost btn-sm"
							onclick={() => cooker.saveCopy()}
							disabled={!!cooker.busy}>Save a copy…</button
						>
					</div>
				</div>
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

		<!-- Signing and publishing -->
		<section class="flex flex-col gap-2">
			<h3 class="font-semibold">Signing and publishing</h3>
			<ModAccount />
		</section>
	</div>
</div>

<PublishDialog bind:open={publishOpen} />
