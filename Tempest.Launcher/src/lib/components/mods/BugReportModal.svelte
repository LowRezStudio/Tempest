<script lang="ts">
	import { Bug, CircleCheck, Lock, MessageCircleQuestion } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { reportRepoBug } from "$lib/queries/repo";
	import { compareVersions } from "$lib/utils/version";

	interface Props {
		open?: boolean;
		modId: string | null;
		versions?: string[];
		defaultVersion?: string;
		defaultGameVersion?: string;
	}

	let {
		open = $bindable(false),
		modId,
		versions = [],
		defaultVersion = "",
		defaultGameVersion = "",
	}: Props = $props();

	const MIN_BODY = 10;
	const MAX_BODY = 2000;

	let body = $state("");
	let version = $state("");
	let gameVersion = $state("");
	let sending = $state(false);
	let threadUrl = $state<string | null>(null);
	let sent = $state(false);
	let submitError = $state("");

	const valid = $derived(body.trim().length >= MIN_BODY);
	// The reported version defaults to the installed one, which may not be in the repo's
	// approved list anymore (e.g. revoked) — keep it as an explicit option.
	const versionOptions = $derived(
		[...new Set([defaultVersion, ...versions].filter(Boolean))].sort((a, b) =>
			compareVersions(b, a),
		),
	);

	// Fresh form each time the modal opens (mirror of the site's BugModal fields).
	$effect(() => {
		if (open) reset();
	});

	function reset() {
		body = "";
		version = defaultVersion;
		gameVersion = defaultGameVersion;
		sending = false;
		threadUrl = null;
		sent = false;
		submitError = "";
	}

	async function send(event: SubmitEvent) {
		event.preventDefault();
		if (sending || !modId || !valid) return;
		sending = true;
		submitError = "";
		try {
			const result = await reportRepoBug(modId, {
				body,
				version: version || undefined,
				gameVersion: gameVersion || undefined,
			});
			threadUrl = result.threadUrl;
			sent = true;
		} catch (error) {
			// Server messages (e.g. the one-per-mod-per-day rule) are shown verbatim;
			// anything else falls back to a generic line.
			const raw = error instanceof Error ? error.message : "";
			submitError =
				raw && raw.trim() && !/^HTTP \d{3}$/.test(raw.trim()) ? raw : m.repo_bug_failed();
		} finally {
			sending = false;
		}
	}
</script>

<!-- The shared Modal renders the <form> and routes its submit to the onsubmit prop. -->
<Modal bind:open title={m.repo_bug_title()} class="max-w-lg" onsubmit={send}>
	{#if sent}
		<div class="flex flex-col items-center py-4 text-center">
			<span
				class="bg-success/15 text-success mb-4 flex size-14 items-center justify-center rounded-full"
			>
				<CircleCheck size={28} />
			</span>
			<h4 class="text-base font-bold">{m.repo_bug_sent_title()}</h4>
			<p class="text-base-content/70 mt-1 max-w-xs text-sm">
				{m.repo_bug_sent_description()}
			</p>
			{#if threadUrl}
				<button class="btn btn-primary btn-sm mt-5" onclick={() => openUrl(threadUrl!)}>
					<MessageCircleQuestion size={14} />
					{m.repo_bug_open_thread()}
				</button>
			{/if}
			<button class="btn btn-sm mt-3" type="button" onclick={() => (open = false)}>
				{m.repo_bug_done()}
			</button>
		</div>
	{:else}
		{#if modId}
			<span class="text-base-content/50 block font-mono text-xs">{modId}</span>
		{/if}

		<div class="flex flex-col gap-4">
			{#if versions.length > 0}
				<label class="flex items-center justify-between gap-3">
					<span class="text-sm font-semibold">{m.repo_bug_version()}</span>
					<select class="select select-sm" bind:value={version}>
						<option value="">{m.repo_bug_version_any()}</option>
						{#each versionOptions as v (v)}
							<option value={v}>v{v}</option>
						{/each}
					</select>
				</label>
			{/if}

			<label class="flex flex-col gap-1">
				<span class="flex items-baseline justify-between">
					<span class="text-sm font-semibold">{m.repo_bug_game_patch()}</span>
					<span class="text-base-content/50 text-xs">{m.repo_bug_optional()}</span>
				</span>
				<input
					type="text"
					class="input input-sm input-bordered font-mono"
					maxlength="32"
					placeholder={m.repo_bug_game_patch_placeholder()}
					bind:value={gameVersion}
				/>
			</label>

			<label class="flex flex-col gap-1">
				<span class="flex items-baseline justify-between">
					<span class="text-sm font-semibold">{m.repo_bug_body_label()}</span>
					<span
						class="text-base-content/50 text-xs"
						class:text-error={!valid && submitError}
						>{body.trim().length < MIN_BODY
							? m.repo_bug_body_minimum({ min: MIN_BODY })
							: `${body.length}/${MAX_BODY}`}</span
					>
				</span>
				<textarea
					class="textarea textarea-bordered h-24 w-full resize-y text-sm"
					maxlength={MAX_BODY}
					placeholder={m.repo_bug_body_placeholder()}
					bind:value={body}></textarea>
			</label>

			{#if submitError}
				<p class="bg-error/10 text-error rounded-lg px-3 py-2 text-sm" role="alert">
					{submitError}
				</p>
			{/if}

			<p class="text-base-content/55 flex items-center gap-1.5 text-xs">
				<Lock size={12} />
				{m.repo_bug_anonymous()}
			</p>
		</div>

		{#snippet actions()}
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}>
				{m.common_cancel()}
			</button>
			<button class="btn btn-primary btn-sm" type="submit" disabled={sending || !valid}>
				{#if sending}
					<span class="loading loading-spinner loading-xs"></span>
				{:else}
					<Bug size={14} />
				{/if}
				{m.repo_bug_send()}
			</button>
		{/snippet}
	{/if}
</Modal>
