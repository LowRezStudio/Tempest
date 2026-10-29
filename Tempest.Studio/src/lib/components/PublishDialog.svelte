<!--
	Publish the built mod to the mod repository (`tempest mod publish --json`),
	like the launcher: the listing (summary, tags, source link), the game
	patches it works with (a list, or a range), what changed, and the outcome:
	published, queued for review, or refused with the reasons. A missing or
	refused API key asks for one in place, then publishes.
-->
<script lang="ts">
	import { CheckCircle2, ExternalLink, KeyRound, Send } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { basename } from "$lib/studio/format";
	import { MAX_SUMMARY, type PublishResult } from "$lib/studio/modmeta";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let busy = $state(false);
	let error = $state("");
	let result = $state<PublishResult | null>(null);
	let needLogin = $state(false);
	let apiKey = $state("");
	let loginError = $state("");

	const m = $derived(cooker.meta);
	const repo = $derived(cooker.account?.repo.url ?? "https://mods.lowrezstudio.com");
	const patches = $derived(
		m.patches
			.split(",")
			.map((p) => p.trim())
			.filter(Boolean),
	);
	const hasCompat = $derived(patches.length > 0 || !!m.patchFrom.trim());
	const summaryOk = $derived(!!m.summary.trim() && m.summary.length <= MAX_SUMMARY);
	const canPublish = $derived(hasCompat && summaryOk && !!cooker.cooked?.signer && !busy);

	$effect(() => {
		if (!open) return;
		error = "";
		result = null;
		needLogin = false;
		loginError = "";
		void cooker.loadAccount();
	});

	async function publish() {
		if (!canPublish) return;
		error = "";
		result = null;
		busy = true;
		try {
			result = await cooker.publish();
		} catch (error_) {
			const message = error_ instanceof Error ? error_.message : String(error_);
			if (message === "needs-login") needLogin = true;
			else error = message;
		} finally {
			busy = false;
		}
	}

	async function login() {
		if (!apiKey.trim()) return;
		busy = true;
		loginError = "";
		const why = await cooker.login(apiKey);
		busy = false;
		if (why) {
			loginError = why;
			return;
		}
		apiKey = "";
		needLogin = false;
		await publish();
	}

	const view = (id: string) => invoke("shell:open-url", { url: `${repo}/mods/${id}` });
</script>

<Modal bind:open title="Publish {m.name.trim() || cooker.modId}" width="max-w-lg">
	{#if result?.state === "done"}
		<div class="flex flex-col items-center gap-3 py-4 text-center text-sm">
			<CheckCircle2 size={40} class="text-success" />
			<p class="font-semibold">
				{result.modId ?? cooker.modId}
				{result.version ?? m.version} is published
			</p>
			{#each result.warnings as w, i (i)}<p class="text-warning text-xs">{w}</p>{/each}
			<button
				class="btn btn-primary btn-sm"
				onclick={() => view(result?.modId ?? cooker.modId)}
			>
				View it online <ExternalLink size={13} />
			</button>
		</div>
	{:else}
		<div class="flex flex-col gap-3 text-sm">
			<p class="text-xs opacity-60">
				Uploads <span class="font-mono"
					>{cooker.cooked ? basename(cooker.cooked.path) : ""}</span
				>
				to {repo.replace(/^https?:\/\//, "")}. A new mod is reviewed before it is listed.
			</p>
			{#if cooker.cooked && !cooker.cooked.signer}
				<p class="text-warning text-xs">
					The package is not signed. Create your signing key (Signing and publishing),
					then build again.
				</p>
			{/if}
			{#if needLogin}
				<div class="bg-base-200 rounded-lg p-3">
					<p class="flex items-center gap-2 text-xs font-semibold">
						<KeyRound size={14} /> Log in to the mod repository
					</p>
					<p class="mt-1 text-xs opacity-60">
						Paste an API key from your account settings on the website (<button
							class="link"
							onclick={() =>
								invoke("shell:open-url", { url: `${repo}/settings#api-keys` })}
							>create one</button
						>).
					</p>
					<form
						class="mt-2 flex gap-1"
						onsubmit={(e) => {
							e.preventDefault();
							void login();
						}}
					>
						<input
							type="password"
							class="input input-sm min-w-0 flex-1 font-mono"
							placeholder="API key (tm_live_…)"
							bind:value={apiKey}
						/>
						<button class="btn btn-primary btn-sm" disabled={busy || !apiKey.trim()}
							>Log in and publish</button
						>
					</form>
					{#if loginError}<p class="text-error mt-1 text-xs">{loginError}</p>{/if}
				</div>
			{/if}
			<label class="flex flex-col gap-1">
				<span class="flex justify-between text-xs opacity-70"
					><span>Summary (one line, shown in the list)</span><span
						class={m.summary.length > MAX_SUMMARY ? "text-error" : ""}
						>{m.summary.length}/{MAX_SUMMARY}</span
					></span
				>
				<input
					class="input input-sm w-full {m.summary.trim() ? '' : 'input-warning'}"
					maxlength={MAX_SUMMARY}
					placeholder="What the mod does, in a sentence"
					bind:value={cooker.meta.summary}
				/>
			</label>
			<div class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Game patches it works with</span>
				<input
					class="input input-sm w-full font-mono {hasCompat ? '' : 'input-warning'}"
					placeholder="8.1, 8.2"
					bind:value={cooker.meta.patches}
				/>
				<span class="text-[11px] opacity-60">Comma-separated, or a range:</span>
				<div class="grid grid-cols-2 gap-2">
					<input
						class="input input-sm font-mono"
						placeholder="from (e.g. 8.1)"
						bind:value={cooker.meta.patchFrom}
					/>
					<input
						class="input input-sm font-mono"
						placeholder="to (empty: and later)"
						bind:value={cooker.meta.patchTo}
					/>
				</div>
			</div>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">What changed in this version</span>
				<textarea
					class="textarea textarea-sm h-20 w-full"
					placeholder="Optional for the first version"
					bind:value={cooker.meta.changelog}></textarea>
			</label>
			{#if error}<p class="text-error text-xs">{error}</p>{/if}
			{#if result?.state === "failed"}
				<div class="bg-error/10 rounded-lg px-3 py-2 text-xs">
					<p class="text-error font-semibold">The repository did not accept it:</p>
					<ul class="mt-1 list-inside list-disc">
						{#each result.errors as e, i (i)}<li>{e}</li>{/each}
					</ul>
				</div>
			{:else if result}
				<p class="text-info text-xs">
					Uploaded: {result.modId ?? cooker.modId}
					{result.version ?? m.version} is queued for review; your dashboard on the website
					shows the outcome.
				</p>
			{/if}
			{#if result?.warnings.length}
				<ul class="text-warning list-inside list-disc text-xs">
					{#each result.warnings as w, i (i)}<li>{w}</li>{/each}
				</ul>
			{/if}
			<div class="flex justify-end gap-2">
				<button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => (open = false)}
					>Close</button
				>
				<button class="btn btn-primary btn-sm" disabled={!canPublish} onclick={publish}>
					{#if busy}<span class="loading loading-xs loading-spinner"></span>{:else}<Send
							size={14}
						/>{/if}
					Publish
				</button>
			</div>
		</div>
	{/if}
</Modal>
