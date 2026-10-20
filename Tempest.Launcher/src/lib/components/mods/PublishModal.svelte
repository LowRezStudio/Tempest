<script lang="ts">
	import { CheckCircle2, KeyRound, Upload } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import {
		isRepoAuthError,
		repoLogin,
		repoPublish,
		type RepoPublishResult,
	} from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { converter } from "$lib/stores/converter.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { modRepositoryURL } from "$lib/stores/settings.svelte";

	interface Props {
		open?: boolean;
		onpublished?: (result: RepoPublishResult) => void;
	}

	let { open = $bindable(false), onpublished }: Props = $props();

	const repoHost = $derived(
		modRepositoryURL.value.replace(/^https?:\/\//, "").replace(/\/$/, ""),
	);

	// Form state; reset per open so a fresh publish starts clean.
	let compatPatches = $state("");
	let compatFrom = $state("");
	let compatTo = $state("");
	let changelog = $state("");
	let compatTouched = $state(false);
	let busy = $state(false);
	let publishError = $state("");
	let result = $state<RepoPublishResult | null>(null);

	// Login state.
	let needLogin = $state(false);
	let apiKey = $state("");
	let loginBusy = $state(false);
	let loginError = $state("");

	$effect(() => {
		if (!open) return;
		publishError = "";
		result = null;
		needLogin = false;
		apiKey = "";
		loginError = "";
		// Suggest the user's own game patch as compatible version, until they edit it.
		if (!compatTouched && !compatPatches && !compatFrom) {
			const patch = Object.values(instanceMap.value)
				.map((inst) => inst.version)
				.filter((v): v is string => !!v)
				.sort()
				.at(-1);
			if (patch) compatPatches = patch;
		}
	});

	let parsedPatches = $derived(
		compatPatches
			.split(",")
			.map((t) => t.trim())
			.filter(Boolean),
	);
	let canPublish = $derived(
		(parsedPatches.length > 0 || !!compatFrom.trim()) && !!converter.summary.trim(),
	);

	async function publish() {
		if (!converter.cooked || busy) return;
		publishError = "";
		result = null;
		if (parsedPatches.length === 0 && !compatFrom.trim()) {
			publishError = m.converter_compat_required();
			return;
		}
		if (!converter.summary.trim()) {
			publishError = m.converter_summary_required();
			return;
		}
		busy = true;
		try {
			const r = await repoPublish(converter.cooked.stagedPath, {
				patches: parsedPatches,
				rangeFrom: compatFrom.trim() || undefined,
				rangeTo: compatTo.trim() || undefined,
				summary: converter.summary.trim(),
				tags: converter.tags.trim(),
				changelog: changelog.trim() || undefined,
			});
			result = r;
			onpublished?.(r);
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			if (isRepoAuthError(message)) {
				needLogin = true;
			} else {
				publishError = message;
			}
		} finally {
			busy = false;
		}
	}

	async function login() {
		if (loginBusy || !apiKey.trim()) return;
		loginBusy = true;
		loginError = "";
		try {
			await repoLogin(apiKey.trim());
			apiKey = "";
			needLogin = false;
			await publish();
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			loginError = m.converter_login_failed({ message });
		} finally {
			loginBusy = false;
		}
	}
</script>

<Modal bind:open title={m.converter_publish_section()} class="max-w-lg" dismissible={!busy}>
	{#if result?.state === "done"}
		<div class="flex flex-col items-center gap-3 py-4 text-center">
			<CheckCircle2 size={40} class="text-success" />
			{#if result.modId && result.version}
				<p class="text-base font-bold">
					{m.converter_publish_done({ id: result.modId, version: result.version })}
				</p>
				<button
					class="btn btn-primary btn-sm"
					onclick={() => openUrl(`${modRepositoryURL.value}/mods/${result?.modId ?? ""}`)}
				>
					{m.converter_view_online()}
				</button>
			{/if}
		</div>
	{:else}
		<p class="text-base-content/50 text-xs">{m.converter_publish_hint({ url: repoHost })}</p>

		{#if needLogin}
			<div class="bg-base-300/50 mt-3 rounded-lg p-4">
				<div class="flex items-center gap-2">
					<KeyRound size={16} />
					<span class="text-sm font-semibold">{m.converter_login_title()}</span>
				</div>
				<p class="text-base-content/50 mt-1 text-xs">{m.converter_login_hint()}</p>
				<p class="mt-1 text-xs">
					<button
						class="link hover:underline"
						onclick={() => openUrl(`${repoHost}/login`)}
					>
						{m.converter_create_api_key()}
					</button>
				</p>
				<div class="mt-3 flex items-center gap-2">
					<input
						type="password"
						class="input input-bordered input-sm flex-1"
						placeholder={m.converter_api_key_placeholder()}
						bind:value={apiKey}
						onkeydown={(e) => e.key === "Enter" && login()}
					/>
					<button
						class="btn btn-primary btn-sm"
						disabled={loginBusy || !apiKey.trim()}
						onclick={login}
					>
						{#if loginBusy}
							<span class="loading loading-spinner loading-xs"></span>
							{m.converter_logging_in()}
						{:else}
							{m.converter_login_btn()}
						{/if}
					</button>
				</div>
				{#if loginError}
					<p class="text-error mt-2 text-xs">{loginError}</p>
				{/if}
			</div>
		{/if}

		<label class="form-control mt-4 gap-1">
			<span class="text-sm font-semibold">{m.converter_summary_label()}</span>
			<input
				type="text"
				class="input input-bordered"
				class:input-error={!converter.summary.trim()}
				bind:value={converter.summary}
				placeholder={m.converter_summary_placeholder()}
			/>
		</label>

		<div class="mt-3">
			<span class="text-sm font-semibold">{m.converter_compat_label()}</span>
			<input
				type="text"
				class="input input-bordered mt-1 w-full font-mono"
				class:input-error={parsedPatches.length === 0 && !compatFrom.trim()}
				bind:value={compatPatches}
				oninput={() => (compatTouched = true)}
				placeholder={m.converter_compat_placeholder()}
			/>
			<p class="text-base-content/50 mt-1 text-xs">{m.converter_compat_hint()}</p>
			<div class="mt-2 grid grid-cols-2 gap-2">
				<label class="form-control gap-1">
					<span class="text-xs opacity-60">{m.converter_range_from_label()}</span>
					<input
						type="text"
						class="input input-bordered input-sm font-mono"
						bind:value={compatFrom}
						oninput={() => (compatTouched = true)}
						placeholder={m.converter_compat_from_placeholder()}
					/>
				</label>
				<label class="form-control gap-1">
					<span class="text-xs opacity-60">{m.converter_range_to_label()}</span>
					<input
						type="text"
						class="input input-bordered input-sm font-mono"
						bind:value={compatTo}
						placeholder={m.converter_compat_to_placeholder()}
					/>
				</label>
			</div>
		</div>

		<label class="form-control mt-3 gap-1">
			<span class="text-sm font-semibold">{m.converter_changelog_label()}</span>
			<textarea
				class="textarea textarea-bordered h-20 w-full"
				bind:value={changelog}
				placeholder={m.converter_changelog_placeholder()}></textarea>
		</label>

		{#if publishError}
			<p class="text-error mt-3 text-sm">{publishError}</p>
		{/if}

		{#if result?.state === "failed"}
			<div class="bg-error/10 mt-3 rounded-lg px-3 py-2 text-sm">
				<p class="text-error font-semibold">{m.converter_publish_rejected()}</p>
				<ul class="text-error/80 mt-1 list-inside list-disc">
					{#each result.errors as item}
						<li>{item}</li>
					{/each}
				</ul>
			</div>
		{:else if result}
			<p class="text-info mt-3 text-sm">{m.converter_publish_queued()}</p>
		{/if}

		{#if result?.warnings.length}
			<div class="bg-warning/10 mt-2 rounded-lg px-3 py-2">
				<p class="text-warning text-xs font-semibold">{m.converter_publish_warnings()}</p>
				<ul class="text-warning/80 mt-1 list-inside list-disc text-xs">
					{#each result.warnings as warning}
						<li>{warning}</li>
					{/each}
				</ul>
			</div>
		{/if}

		<div class="mt-4 flex justify-end gap-2">
			<button
				class="btn btn-ghost"
				type="button"
				disabled={busy}
				onclick={() => (open = false)}
			>
				{m.common_cancel()}
			</button>
			<button
				class="btn btn-primary"
				type="button"
				disabled={!canPublish || busy}
				onclick={publish}
			>
				{#if busy}
					<span class="loading loading-spinner loading-xs"></span>
					{m.converter_publishing()}
				{:else}
					<Upload size={16} />
					{m.converter_publish_btn()}
				{/if}
			</button>
		</div>
	{/if}
</Modal>
