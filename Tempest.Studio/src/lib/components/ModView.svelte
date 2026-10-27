<!--
	The mod cooker: the game files the mod changes (each with where its edited
	content comes from, the original it is compared with, and what the delta
	holds), other files (Lua, DLLs…), the details, then cook into a signed
	.tempest and save, install or publish it.
-->
<script lang="ts">
	import {
		CircleAlert,
		CircleCheck,
		Download,
		FilePlus2,
		FileUp,
		FolderPlus,
		Hammer,
		ImagePlus,
		KeyRound,
		Plus,
		RefreshCw,
		Send,
		Sparkles,
		Trash2,
		X,
	} from "@lucide/svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker, type Entry } from "$lib/studio/cooker.svelte";
	import { basename } from "$lib/studio/format";
	import { studio } from "$lib/studio/state.svelte";
	import type { CookSource } from "../../../electron/src/ipc/contract";

	let keyId = $state("");
	let apiKey = $state("");
	let logEl = $state<HTMLDivElement | null>(null);

	$effect(() => {
		if (studio.workspace) void cooker.refresh();
	});
	$effect(() => {
		void cooker.log.length;
		requestAnimationFrame(() => logEl?.scrollTo({ top: logEl.scrollHeight }));
	});

	const m = $derived(cooker.meta);

	function when(t?: number): string {
		return t ? new Date(t * 1000).toLocaleString() : "";
	}

	function sourceLabel(s: CookSource): string {
		if (s.origin === "unsaved") return "Unsaved edits in the Studio";
		if (s.origin === "saved") return "Saved in the game folder";
		const parts = s.source.split(/[/\\]/);
		const folder = parts.at(-2) ?? "";
		return `Exported copy ${parts.at(-1) ?? ""} in …/${folder}`;
	}

	function summary(e: Entry): { text: string; tone: string } {
		const c = e.chosen;
		if (!c) return { text: "", tone: "" };
		if (e.choice.mode === "delta" && c.base.kind === "none") {
			return { text: "needs its original", tone: "badge-warning" };
		}
		const p = cooker.previews[c.id];
		if (!p) return { text: cooker.previewing ? "…" : "", tone: "badge-ghost" };
		if (p.error) return { text: "error", tone: "badge-error" };
		if (p.mode === "whole") return { text: "whole file", tone: "badge-ghost" };
		const n = (p.changed ?? 0) + (p.added ?? 0) + (p.deleted ?? 0);
		if (n === 0) return { text: "no changes", tone: "badge-ghost" };
		const parts = [
			p.changed ? `${p.changed} changed` : "",
			p.added ? `${p.added} added` : "",
			p.deleted ? `${p.deleted} removed` : "",
		].filter(Boolean);
		return { text: parts.join(", "), tone: "badge-success" };
	}
</script>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex max-w-6xl flex-col gap-4 p-4 text-sm">
		<header class="flex flex-wrap items-center gap-3">
			<Hammer size={22} class="text-primary" />
			<div class="min-w-0 flex-1">
				<h2 class="text-lg font-semibold">Mod cooker</h2>
				<p class="text-xs opacity-70">
					Your mod is the list of game files it changes, each shipped as a delta (only
					what you changed, so mods on the same file combine), plus scripts or other
					files. Cooked into a signed <code>.tempest</code>, then saved, installed or
					published.
				</p>
			</div>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => cooker.refresh()}
				title="Look for changes again"
			>
				<RefreshCw size={14} /> Refresh
			</button>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => cooker.newMod()}
				title="Clear the details and choices"
			>
				<Sparkles size={14} /> New mod
			</button>
		</header>

		{#if cooker.cli && !cooker.cli.available}
			<div class="alert alert-error text-xs">
				tempest-cli was not found. Build it with <code>dotnet build Tempest.slnx</code>
				(dev) or set
				<code>TEMPEST_CLI</code>.
			</div>
		{:else if cooker.cli && !cooker.cli.hasKey}
			<div class="alert alert-warning flex flex-wrap items-center gap-2 text-xs">
				<KeyRound size={14} />
				<span class="flex-1"
					>No signing key yet: packages with code (Lua, DLLs) must be signed to install.
					Create one (trusted on this machine; to publish, verify it on
					mods.lowrezstudio.com).</span
				>
				<input
					class="input input-xs w-48"
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

		<div class="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
			<!-- Game files -->
			<section
				class="border-base-300 bg-base-200/40 flex min-w-0 flex-col gap-2 rounded-lg border p-3"
			>
				<div class="flex flex-wrap items-center gap-2">
					<h3 class="font-semibold">Game files this mod changes</h3>
					<span class="text-xs opacity-60"
						>{cooker.included.length} of {cooker.entries.length}</span
					>
					<span class="flex-1"></span>
					{#if cooker.previewing}<span class="loading loading-xs loading-spinner"
						></span>{/if}
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => cooker.addModified()}
						title="A .upk or .dat edited elsewhere (or saved as a copy earlier): it is compared with the game file of the same name"
						><FilePlus2 size={12} /> Add a modified file…</button
					>
				</div>
				{#if cooker.entries.length === 0}
					<p class="py-4 text-center text-xs opacity-60">
						No changes yet. Edit packages or the assembly here (the edits can stay
						unsaved), export copies with Save as, or add a modified file.
					</p>
				{/if}
				{#each cooker.entries as e (e.target)}
					{@const s = summary(e)}
					{@const p = e.chosen ? cooker.previews[e.chosen.id] : undefined}
					<div
						class="border-base-300 bg-base-100 rounded-md border p-2 {e.choice.on
							? ''
							: 'opacity-60'}"
					>
						<div class="flex items-center gap-2">
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								checked={e.choice.on}
								onchange={(ev) =>
									cooker.setChoice(e.target, { on: ev.currentTarget.checked })}
								aria-label="Include {e.rel}"
							/>
							<span
								class="min-w-0 flex-1 truncate font-mono text-xs font-semibold"
								title={e.target}>{e.rel.replace(/^ChaosGame\//, "")}</span
							>
							{#if s.text}<span class="badge badge-sm {s.tone} shrink-0"
									>{s.text}</span
								>{/if}
						</div>
						{#if e.choice.on}
							<div class="mt-1.5 ml-7 flex flex-col gap-1.5 text-xs">
								<!-- Where the edited content comes from -->
								{#if e.sources.length > 1}
									<fieldset class="flex min-w-0 flex-col gap-0.5">
										<legend class="mb-0.5 opacity-60"
											>Take the changes from</legend
										>
										{#each e.sources as src (src.id)}
											<label class="flex min-w-0 items-center gap-2">
												<input
													type="radio"
													class="radio radio-xs"
													name="src-{e.target}"
													checked={e.chosen?.id === src.id}
													onchange={() =>
														cooker.setChoice(e.target, {
															source: src.id,
														})}
												/>
												<span class="truncate" title={src.source}
													>{sourceLabel(src)}</span
												>
												{#if src.origin === "copy"}
													<span class="shrink-0 opacity-50"
														>{when(src.when)}</span
													>
													<button
														class="btn btn-ghost btn-xs btn-square shrink-0"
														onclick={() => cooker.forgetCopy(src)}
														title="Remove this copy from the list (the file stays)"
														aria-label="Forget this copy"
														><X size={11} /></button
													>
												{/if}
											</label>
										{/each}
									</fieldset>
								{:else if e.chosen}
									<div class="flex min-w-0 items-center gap-2 opacity-80">
										<span class="truncate" title={e.chosen.source}
											>{sourceLabel(e.chosen)}</span
										>
										{#if e.chosen.origin === "copy"}
											<span class="shrink-0 opacity-60"
												>{when(e.chosen.when)}</span
											>
											<button
												class="btn btn-ghost btn-xs btn-square shrink-0"
												onclick={() =>
													e.chosen && cooker.forgetCopy(e.chosen)}
												title="Remove this copy from the list (the file stays)"
												aria-label="Forget this copy"
												><X size={11} /></button
											>
										{/if}
									</div>
								{/if}
								<!-- What it is compared with -->
								{#if e.choice.mode === "delta" && e.chosen}
									{#if e.chosen.base.kind === "recorded"}
										<div class="flex items-center gap-1.5 opacity-70">
											<CircleCheck size={12} class="text-success" /> Compared with
											its original, kept by the Studio{e.chosen.base.recorded
												? ` (${when(e.chosen.base.recorded)})`
												: ""}
										</div>
									{:else if e.chosen.base.kind === "backup"}
										<div
											class="flex items-center gap-1.5 opacity-70"
											title={e.chosen.base.path}
										>
											<CircleCheck size={12} class="text-success" /> Compared with
											the launcher's backup of the original
										</div>
									{:else}
										<div
											class="bg-warning/10 border-warning/40 flex flex-wrap items-center gap-2 rounded border p-1.5"
										>
											<CircleAlert size={13} class="text-warning" />
											<span class="flex-1"
												>No original of this game file is known, so the
												delta has nothing to compare with.</span
											>
											<button
												class="btn btn-xs"
												onclick={() => cooker.useGameFile(e.target)}
												>The game file is unmodified</button
											>
											<button
												class="btn btn-xs"
												onclick={() => cooker.pickOriginal(e.target)}
												>Pick the original…</button
											>
										</div>
									{/if}
								{/if}
								{#if p?.error}
									<div class="text-error">
										{p.error.replace(/^needs-original: /, "")}
									</div>
								{/if}
								{#if p?.warnings?.length}
									<div class="text-warning">{p.warnings.join("; ")}</div>
								{/if}
								{#if p?.changes?.length}
									<details>
										<summary class="cursor-pointer opacity-70"
											>What changes ({p.changes.length}{p.changes.length >=
											300
												? "+"
												: ""})</summary
										>
										<ul
											class="mt-1 max-h-40 overflow-y-auto font-mono text-[11px]"
										>
											{#each p.changes as c, i (i)}
												<li class="truncate">
													<span
														class={c.op === "add"
															? "text-success"
															: c.op === "delete"
																? "text-error"
																: "text-info"}
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
									</details>
								{/if}
								<label class="flex items-center gap-2 opacity-70">
									<input
										type="checkbox"
										class="checkbox checkbox-xs"
										checked={e.choice.mode === "whole"}
										onchange={(ev) =>
											cooker.setChoice(e.target, {
												mode: ev.currentTarget.checked ? "whole" : "delta",
											})}
									/>
									Ship the whole file instead (replaces it; conflicts with any other
									mod changing it)
								</label>
							</div>
						{/if}
					</div>
				{/each}

				<div class="mt-2 flex flex-wrap items-center gap-1">
					<h3 class="font-semibold">Other files</h3>
					<span class="text-xs opacity-60">scripts, DLLs, anything shipped as is</span>
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => cooker.addFiles()}
						><FileUp size={12} /> Files…</button
					>
					<button class="btn btn-ghost btn-xs" onclick={() => cooker.addFolder()}
						><FolderPlus size={12} /> Folder…</button
					>
				</div>
				{#each cooker.extras as x, i (i)}
					<div class="flex items-center gap-1">
						<span
							class="w-40 shrink-0 truncate font-mono text-xs opacity-70"
							title={x.src}>{basename(x.src)}</span
						>
						<span class="opacity-40">→</span>
						<input
							class="input input-xs min-w-0 flex-1 font-mono"
							bind:value={x.dest}
							title="Path inside the mod (scripts/…, dlls/64/…, files/ChaosGame/…)"
						/>
						<button
							class="btn btn-ghost btn-xs btn-square"
							onclick={() =>
								(cooker.extras = cooker.extras.filter((_, k) => k !== i))}
							aria-label="Remove"><Trash2 size={11} /></button
						>
					</div>
				{:else}
					<p class="text-xs opacity-50">None.</p>
				{/each}
			</section>

			<!-- Details -->
			<section
				class="border-base-300 bg-base-200/40 flex min-w-0 flex-col gap-2 rounded-lg border p-3"
			>
				<h3 class="font-semibold">Details</h3>
				<div class="grid grid-cols-[6rem_minmax(0,1fr)] items-center gap-2">
					<label class="text-xs opacity-70" for="mod-id">Id</label>
					<input
						id="mod-id"
						class="input input-sm w-full font-mono {m.id && !cooker.idValid
							? 'input-error'
							: ''}"
						bind:value={cooker.meta.id}
						placeholder="my-champion"
						title="Lowercase letters, digits, . _ - (2 to 64)"
					/>
					<label class="text-xs opacity-70" for="mod-name">Name</label>
					<input
						id="mod-name"
						class="input input-sm w-full"
						bind:value={cooker.meta.name}
						placeholder="My Champion"
					/>
					<label class="text-xs opacity-70" for="mod-version">Version</label>
					<input
						id="mod-version"
						class="input input-sm w-full font-mono"
						bind:value={cooker.meta.version}
					/>
					<span class="text-xs opacity-70">Icon</span>
					<div class="flex min-w-0 items-center gap-2">
						<button class="btn btn-sm" onclick={() => cooker.pickIcon()}
							><ImagePlus size={14} /> Choose…</button
						>
						{#if m.icon}
							<span class="truncate font-mono text-xs" title={m.icon}
								>{basename(m.icon)}</span
							>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => (cooker.meta.icon = null)}
								aria-label="Remove the icon"><X size={12} /></button
							>
						{/if}
					</div>
				</div>
				<div>
					<div class="mb-1 flex items-center text-xs opacity-70">
						Authors <span class="flex-1"></span>
						<button
							class="btn btn-ghost btn-xs"
							onclick={() =>
								(cooker.meta.authors = [
									...cooker.meta.authors,
									{ name: "", link: "" },
								])}><Plus size={12} /> Add</button
						>
					</div>
					{#each cooker.meta.authors as a, i (i)}
						<div class="mb-1 flex gap-1">
							<input
								class="input input-sm w-32"
								placeholder="name"
								bind:value={a.name}
							/>
							<input
								class="input input-sm min-w-0 flex-1"
								placeholder="link (optional)"
								bind:value={a.link}
							/>
							<button
								class="btn btn-ghost btn-sm btn-square"
								onclick={() =>
									(cooker.meta.authors = cooker.meta.authors.filter(
										(_, k) => k !== i,
									))}
								aria-label="Remove author"><X size={12} /></button
							>
						</div>
					{/each}
				</div>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70"
						>Dependencies (one per line, e.g. tempest-scripting&gt;=1.0.0)</span
					>
					<textarea
						class="textarea textarea-sm w-full font-mono"
						rows="2"
						bind:value={cooker.meta.dependencies}></textarea>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Readme (Markdown)</span>
					<textarea
						class="textarea textarea-sm w-full"
						rows="4"
						bind:value={cooker.meta.readme}></textarea>
				</label>
			</section>
		</div>

		<section class="border-base-300 bg-base-200/40 flex flex-col gap-3 rounded-lg border p-3">
			<div class="flex flex-wrap items-center gap-2">
				<button
					class="btn btn-primary btn-sm"
					onclick={() => cooker.cook()}
					disabled={!cooker.canCook || !!cooker.busy}
				>
					{#if cooker.busy === "Cooking…"}<span class="loading loading-xs loading-spinner"
						></span>{:else}<Hammer size={14} />{/if}
					Cook and save .tempest…
				</button>
				{#if cooker.cooked}
					<span class="min-w-0 truncate font-mono text-xs" title={cooker.cooked.path}
						>{basename(cooker.cooked.path)}</span
					>
					<span
						class="badge badge-sm {cooker.cooked.signer
							? 'badge-success'
							: 'badge-warning'}"
						>{cooker.cooked.signer
							? `signed: ${cooker.cooked.signer}`
							: "unsigned"}</span
					>
					<button
						class="btn btn-ghost btn-xs"
						onclick={() =>
							cooker.cooked &&
							invoke("shell:show-item", { path: cooker.cooked.path })}>Show</button
					>
					<span class="flex-1"></span>
					<button
						class="btn btn-sm"
						onclick={() => cooker.install()}
						disabled={!!cooker.busy}
						title="tempest mod install into this workspace's game folder (replacing an earlier version)"
					>
						<Download size={14} /> Install here
					</button>
					<button
						class="btn btn-ghost btn-sm"
						onclick={() => cooker.install(true)}
						disabled={!!cooker.busy}>Install elsewhere…</button
					>
				{/if}
			</div>
			{#if !cooker.canCook && cooker.cli?.available}
				<ul class="list-inside list-disc text-xs opacity-70">
					{#if !cooker.idValid}<li>
							Give the mod an id (lowercase letters, digits, . _ -).
						</li>{/if}
					{#if !m.name.trim()}<li>Give the mod a name.</li>{/if}
					{#if cooker.included.length === 0 && cooker.extras.length === 0}<li>
							Include at least one game file or other file.
						</li>{/if}
					{#each cooker.missingOriginals as e (e.target)}
						<li>{basename(e.target)} needs its original (see above).</li>
					{/each}
				</ul>
			{/if}
			{#if cooker.cooked}
				<details class="border-base-300 rounded border p-2">
					<summary class="cursor-pointer text-xs font-semibold"
						><Send size={12} class="inline" /> Publish to mods.lowrezstudio.com</summary
					>
					<div class="mt-2 grid gap-2 sm:grid-cols-2">
						<input
							class="input input-sm"
							placeholder="first compatible patch (e.g. 8.1)"
							bind:value={cooker.meta.patchFrom}
						/>
						<input
							class="input input-sm"
							placeholder="last compatible patch (empty: and later)"
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
								placeholder="API key (tm_live_…), only once per machine"
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
				<div
					bind:this={logEl}
					class="bg-base-300 selectable max-h-56 overflow-auto rounded p-2 font-mono text-[11px] whitespace-pre-wrap"
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
			{/if}
		</section>
	</div>
</div>
