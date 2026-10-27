<script lang="ts">
	import { ExternalLink, Link2, Table2 } from "@lucide/svelte";
	import { assembly, isMessageColumn, refOf } from "$lib/studio/assembly.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
	import type { McsField } from "../../../electron/src/ipc/contract";

	let editing = $state<string | null>(null);
	/** `${id}|${lang}` of the message being edited. */
	let editingMessage = $state<string | null>(null);

	const fields = $derived(assembly.fields);
	const index = $derived(assembly.selected);
	const isLang = $derived(assembly.page?.doc !== "assembly");

	/** The row's own id field (DATA_SET_BOTS → BOT_ID), for "Used by". */
	const idField = $derived.by(() => {
		const table = assembly.page?.table;
		if (!table || !fields) return null;
		const stem = table.replace(/^DATA_SET_/, "");
		const candidates = new Set([
			`${stem}_ID`,
			stem.endsWith("IES") ? `${stem.slice(0, -3)}Y_ID` : "",
			stem.endsWith("ES") ? `${stem.slice(0, -2)}_ID` : "",
			stem.endsWith("S") ? `${stem.slice(0, -1)}_ID` : "",
		]);
		return fields.find((f) => candidates.has(f.name) && typeof f.value === "number") ?? null;
	});
	let usages = $state<{ path: [string, number][]; field: string; label: string }[] | null>(null);
	let usagesTruncated = $state(false);
	let usagesLoading = $state(false);
	$effect(() => {
		void index;
		void assembly.page?.table;
		usages = null;
	});

	async function findUsages() {
		if (!idField || typeof idField.value !== "number") return;
		usagesLoading = true;
		try {
			const r = await assembly.usages(idField.name, idField.value);
			const own = JSON.stringify(assembly.rowPath(index ?? -1));
			usages = r.usages.filter((u) => JSON.stringify(u.path) !== own);
			usagesTruncated = r.truncated;
		} finally {
			usagesLoading = false;
		}
	}

	function goTo(u: { path: [string, number][] }, newTab: boolean) {
		const last = u.path.at(-1);
		if (!last || !assembly.page) return;
		if (newTab) tabs.openInNewTab();
		void assembly.openTable(assembly.page.doc, last[0], u.path.slice(0, -1), last[1]);
	}

	function text(f: McsField): string {
		if (f.value === null) return "";
		if (typeof f.value === "object") return `${f.value.rows} rows`;
		return String(f.value);
	}

	function isFlag(f: McsField): boolean {
		return (
			f.kind === "string" && f.name.endsWith("_FLAG") && (f.value === "y" || f.value === "n")
		);
	}

	function validValue(f: McsField): string | null {
		if (!f.name.endsWith("_VALUE_ID") || typeof f.value !== "number" || !assembly.validValues) {
			return null;
		}
		return assembly.validValues[String(f.value)]?.text ?? null;
	}

	async function commit(f: McsField, value: string) {
		editing = null;
		if (index === null || value === text(f)) return;
		await assembly.setCell(index, f.name, value);
	}

	async function toggleFlag(f: McsField) {
		if (index === null) return;
		await assembly.setCell(index, f.name, f.value === "y" ? "n" : "y");
	}

	async function openNested(f: McsField) {
		const p = assembly.page;
		if (!p || index === null) return;
		await assembly.openTable(p.doc, f.name, [...p.path, [p.table, index]]);
	}

	async function commitMessage(id: number, lang: string, value: string) {
		editingMessage = null;
		if (value === (assembly.message(id, lang) ?? "")) return;
		await assembly.setMessage(id, lang, value);
	}
</script>

<div class="pane-title">
	Row
	{#if index !== null}<span class="font-mono normal-case opacity-60">#{index}</span>{/if}
</div>
{#if fields && index !== null}
	<div class="min-h-0 flex-1 overflow-y-auto p-2 text-xs">
		{#if idField && !isLang}
			<div class="border-base-300 mb-2 rounded border p-2">
				<div class="flex items-center gap-2">
					<Link2 size={12} class="opacity-60" />
					<span class="font-semibold">Used by</span>
					<span class="font-mono opacity-60">{idField.name} {idField.value}</span>
					<span class="flex-1"></span>
					{#if usages === null}
						<button
							class="btn btn-ghost btn-xs"
							onclick={findUsages}
							disabled={usagesLoading}
						>
							{#if usagesLoading}<span class="loading loading-xs loading-spinner"
								></span>{/if}
							Find
						</button>
					{:else}
						<span class="opacity-60"
							>{usages.length}{usagesTruncated ? "+" : ""} rows</span
						>
					{/if}
				</div>
				{#if usages && usages.length > 0}
					<ul class="mt-1 max-h-48 overflow-y-auto">
						{#each usages as u, k (k)}
							<li>
								<button
									class="hover:bg-base-200 flex w-full items-center gap-2 rounded px-1 py-0.5 text-left font-mono"
									onclick={(e) => goTo(u, e.ctrlKey || e.metaKey)}
									onauxclick={(e) => e.button === 1 && goTo(u, true)}
									title="Go to the row (Ctrl+click: new tab)"
								>
									<span class="truncate"
										>{u.path
											.map(([t, i]) => `${t.replace(/^DATA_SET_/, "")}[${i}]`)
											.join(" › ")}</span
									>
									<span class="opacity-50">{u.field}</span>
									{#if u.label}<span class="truncate opacity-60">{u.label}</span
										>{/if}
								</button>
							</li>
						{/each}
					</ul>
				{:else if usages}
					<p class="mt-1 opacity-60">Nothing in this file refers to it.</p>
				{/if}
			</div>
		{/if}
		{#each fields as f (f.name)}
			{@const ref = refOf(f.name)}
			{@const vv = validValue(f)}
			<div class="border-base-300/60 border-b py-1">
				<div class="flex items-center gap-2">
					<span
						class="w-44 shrink-0 truncate font-mono opacity-70"
						title="{f.name} ({f.kind})">{f.name}</span
					>
					{#if f.kind === "rowset"}
						<button class="btn btn-ghost btn-xs" onclick={() => openNested(f)}>
							<Table2 size={11} />
							{text(f)}
						</button>
					{:else if isFlag(f)}
						<input
							type="checkbox"
							class="toggle toggle-xs"
							checked={f.value === "y"}
							onchange={() => toggleFlag(f)}
						/>
						<span class="opacity-60">{f.value}</span>
					{:else if editing === f.name}
						{#if f.kind === "string" && String(f.value).length > 60}
							<!-- svelte-ignore a11y_autofocus -->
							<textarea
								class="textarea textarea-xs flex-1 font-mono"
								rows="4"
								autofocus
								value={text(f)}
								onblur={(e) => commit(f, e.currentTarget.value)}
								onkeydown={(e) => {
									if (e.key === "Escape") editing = null;
								}}></textarea>
						{:else}
							<!-- svelte-ignore a11y_autofocus -->
							<input
								class="input input-xs flex-1 font-mono"
								autofocus
								value={text(f)}
								onblur={(e) => commit(f, e.currentTarget.value)}
								onkeydown={(e) => {
									if (e.key === "Enter") e.currentTarget.blur();
									if (e.key === "Escape") editing = null;
								}}
							/>
						{/if}
					{:else}
						<button
							class="hover:bg-base-300 min-w-0 flex-1 truncate rounded px-1 text-left font-mono {f.kind ===
							'blob'
								? 'opacity-50'
								: ''}"
							title={f.kind === "blob" ? "binary" : "click to edit"}
							onclick={() => {
								if (f.kind !== "blob") editing = f.name;
							}}>{text(f)}</button
						>
						{#if ref && typeof f.value === "number"}
							<button
								class="btn btn-ghost btn-xs"
								title="Go to {ref.table}"
								onclick={() => assembly.follow(f.name, f.value)}
								><ExternalLink size={11} /></button
							>
						{/if}
					{/if}
				</div>
				{#if vv}
					<div class="ml-46 pl-1 opacity-60">{vv}</div>
				{/if}
				{#if isMessageColumn(f.name) && typeof f.value === "number" && f.value !== 0}
					<div class="mt-1 ml-2 space-y-0.5 border-l pl-2">
						{#each assembly.languages as lang (lang)}
							{@const m = assembly.messages[String(f.value)]?.[lang]}
							<div class="flex items-start gap-2">
								<span class="w-8 shrink-0 font-mono opacity-50">{lang}</span>
								{#if editingMessage === `${f.value}|${lang}`}
									<!-- svelte-ignore a11y_autofocus -->
									<textarea
										class="textarea textarea-xs flex-1"
										rows="3"
										autofocus
										value={m?.text ?? ""}
										onblur={(e) =>
											commitMessage(
												f.value as number,
												lang,
												e.currentTarget.value,
											)}
										onkeydown={(e) => {
											if (e.key === "Escape") editingMessage = null;
										}}></textarea>
								{:else}
									<button
										class="hover:bg-base-300 min-w-0 flex-1 rounded px-1 text-left whitespace-pre-wrap {m
											? ''
											: 'italic opacity-40'}"
										title={m
											? `${m.name} (click to edit)`
											: "no message in this language (click to add)"}
										onclick={() => (editingMessage = `${f.value}|${lang}`)}
										>{m?.text ?? "—"}</button
									>
									{#if m}
										<button
											class="btn btn-ghost btn-xs"
											title="Open in Lang_{lang}.dat"
											onclick={() =>
												assembly.openMessage(f.value as number, lang)}
											><ExternalLink size={11} /></button
										>
									{/if}
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		{/each}
		{#if isLang}
			<p class="mt-2 opacity-50">
				Messages are looked up by MSG_ID from the assembly's *_MSG_ID columns; keep ids
				unique.
			</p>
		{/if}
	</div>
{:else if index !== null}
	<div class="flex h-full items-center justify-center">
		<span class="loading loading-sm loading-spinner"></span>
	</div>
{:else}
	<div class="flex h-full items-center justify-center p-4 text-center text-sm opacity-60">
		Select a row to see every field, follow its references and edit its messages.
	</div>
{/if}
