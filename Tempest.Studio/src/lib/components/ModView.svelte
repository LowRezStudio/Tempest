<!--
	The mod cooker: details, contents (the workspace's changes + extra files),
	then cook into a signed .tempest and save, install or publish it with
	tempest-cli. One scrolling page, readable at any width.
-->
<script lang="ts">
	import {
		FileUp,
		FolderPlus,
		Hammer,
		ImagePlus,
		KeyRound,
		Plus,
		RefreshCw,
		Send,
		Download,
		Trash2,
		X,
	} from "@lucide/svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { basename } from "$lib/studio/format";
	import { studio } from "$lib/studio/state.svelte";

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
	const unsaved = $derived(cooker.items.filter((i) => i.state === "unsaved").length);
</script>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto flex max-w-5xl flex-col gap-4 p-4 text-sm">
		<header class="flex flex-wrap items-center gap-3">
			<Hammer size={22} class="text-primary" />
			<div class="min-w-0 flex-1">
				<h2 class="text-lg font-semibold">Mod cooker</h2>
				<p class="text-xs opacity-70">
					Package this workspace's changes as a <code>.tempest</code> mod: changed
					packages and
					<code>.dat</code> files go in as deltas (only what changed, so mods on the same file
					combine) or whole, with any extra files (Lua, DLLs). Signed with your key, then saved,
					installed or published.
				</p>
			</div>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => cooker.refresh()}
				title="List the changes again"
			>
				<RefreshCw size={14} /> Refresh
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

		<div class="grid gap-4 lg:grid-cols-2">
			<section
				class="border-base-300 bg-base-200/40 flex flex-col gap-2 rounded-lg border p-3"
			>
				<h3 class="font-semibold">Details</h3>
				<div class="grid grid-cols-[7rem_1fr] items-center gap-2">
					<label class="text-xs opacity-70" for="mod-id">Id</label>
					<input
						id="mod-id"
						class="input input-sm font-mono {m.id && !cooker.idValid
							? 'input-error'
							: ''}"
						bind:value={cooker.meta.id}
						placeholder="my-champion"
						title="Lowercase letters, digits, . _ - (2 to 64)"
					/>
					<label class="text-xs opacity-70" for="mod-name">Name</label>
					<input
						id="mod-name"
						class="input input-sm"
						bind:value={cooker.meta.name}
						placeholder="My Champion"
					/>
					<label class="text-xs opacity-70" for="mod-version">Version</label>
					<input
						id="mod-version"
						class="input input-sm font-mono"
						bind:value={cooker.meta.version}
					/>
					<span class="text-xs opacity-70">Icon</span>
					<div class="flex items-center gap-2">
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
								class="input input-sm w-40"
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

			<section
				class="border-base-300 bg-base-200/40 flex min-w-0 flex-col gap-2 rounded-lg border p-3"
			>
				<h3 class="flex items-center gap-2 font-semibold">
					Contents
					<span class="text-xs font-normal opacity-60"
						>{cooker.selected.length} file(s){unsaved > 0
							? `, ${unsaved} with unsaved edits (taken as they are in the Studio)`
							: ""}</span
					>
				</h3>
				{#if cooker.items.length === 0}
					<p class="text-xs opacity-60">
						No changes yet: edit packages or the assembly, or save a file in place (the
						Studio records its original first, so the mod gets a delta).
					</p>
				{:else}
					<div class="max-h-64 overflow-auto">
						<table class="table-xs table">
							<tbody>
								{#each cooker.items as it (it.path)}
									<tr class={it.on ? "" : "opacity-50"}>
										<td class="w-6"
											><input
												type="checkbox"
												class="checkbox checkbox-xs"
												bind:checked={it.on}
												aria-label="Include {it.rel}"
											/></td
										>
										<td class="max-w-0 truncate font-mono" title={it.path}
											>{it.rel.replace(/^ChaosGame\//, "")}</td
										>
										<td class="w-20">
											<span
												class="badge badge-xs {it.state === 'unsaved'
													? 'badge-warning'
													: 'badge-ghost'}">{it.state}</span
											>
										</td>
										<td class="w-28">
											<select
												class="select select-xs"
												bind:value={it.mode}
												title="Delta: only the changed objects or rows; whole: the full file"
											>
												<option value="delta">delta</option>
												<option value="whole">whole file</option>
											</select>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
				<div class="mt-1 flex items-center gap-1 text-xs opacity-70">
					Extra files <span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => cooker.addFiles()}
						><FileUp size={12} /> Files…</button
					>
					<button class="btn btn-ghost btn-xs" onclick={() => cooker.addFolder()}
						><FolderPlus size={12} /> Folder…</button
					>
				</div>
				{#each cooker.extras as x, i (i)}
					<div class="flex items-center gap-1">
						<span class="w-40 truncate font-mono text-xs opacity-70" title={x.src}
							>{basename(x.src)}</span
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
				{/each}
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
				<p class="text-xs opacity-60">
					{!cooker.idValid
						? "Give a valid id (lowercase, digits, . _ -). "
						: ""}{!m.name.trim() ? "Give a name. " : ""}{cooker.selected.length === 0 &&
					cooker.extras.length === 0
						? "Pick at least one file."
						: ""}
				</p>
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
							>
								<Send size={14} /> Publish
							</button>
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
