<script lang="ts">
	import { KeyRound, ShieldCheck } from "@lucide/svelte";
	import { generateSigningKey, isKeyExistsError, repoLogin, signText } from "$lib/core/mods";
	import { promptSigningKeyId } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";
	import { addToast } from "$lib/stores/ui.svelte";

	const publishingEnabled = $derived(isFlagEnabled("mod-publishing"));

	let hasKey = $state(false);
	let keyId = $state<string | null>(null);
	let keyBusy = $state(false);
	let keyError = $state("");

	let showVerify = $state(false);
	let verifyText = $state("");
	let verifyBusy = $state(false);
	let verifyError = $state("");
	let verifySignature = $state("");

	let hasApiKey = $state(false);
	let maskedKey = $state<string | null>(null);
	let editingApiKey = $state(false);
	let apiKey = $state("");
	let apiBusy = $state(false);
	let apiError = $state("");

	async function load() {
		try {
			const [signing, repo] = await Promise.all([
				invoke("signing:status"),
				invoke("repo:status"),
			]);
			hasKey = signing.hasKey;
			keyId = signing.keyId;
			hasApiKey = repo.hasApiKey;
			maskedKey = repo.masked;
		} catch (error) {
			console.error("Failed to load publishing setup status:", error);
		}
	}

	$effect(() => {
		void load();
	});

	// Asks for the key name in the shared dialog, then generates/overwrites.
	async function createKey() {
		keyError = "";
		const id = await promptSigningKeyId();
		if (!id) return;
		keyBusy = true;
		try {
			await generateSigningKey(id);
			await load();
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			// A key already being present is fine — show the existing one.
			if (isKeyExistsError(message)) {
				await load();
			} else {
				keyError = message;
			}
		} finally {
			keyBusy = false;
		}
	}

	async function saveApiKey() {
		const key = apiKey.trim();
		if (!key || apiBusy) return;
		apiBusy = true;
		apiError = "";
		try {
			await repoLogin(key);
			apiKey = "";
			editingApiKey = false;
			await load();
		} catch (error) {
			apiError = error instanceof Error ? error.message : String(error);
		} finally {
			apiBusy = false;
		}
	}

	async function forgetApiKey() {
		try {
			await invoke("repo:forget");
			await load();
		} catch (error) {
			console.error("Failed to forget API key:", error);
		}
	}

	// Signs the account-page verification text with the default key; the user
	// pastes the resulting signature back on the website to prove key ownership.
	async function verifyKey() {
		const text = verifyText.trim();
		if (!text || verifyBusy) return;
		verifyBusy = true;
		verifyError = "";
		verifySignature = "";
		try {
			verifySignature = await signText(text);
		} catch (error) {
			verifyError = error instanceof Error ? error.message : String(error);
		} finally {
			verifyBusy = false;
		}
	}

	async function copySignature() {
		if (!verifySignature) return;
		try {
			await navigator.clipboard.writeText(verifySignature);
			addToast({
				title: m.settings_signing_key_verify_copied(),
				message: m.settings_signing_key_verify_copied_message(),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to copy signature:", error);
		}
	}
</script>

<div class="form-control gap-1">
	<span class="label-text text-sm">{m.settings_publishing_title()}</span>

	<div class="bg-base-200 rounded-box mt-1 flex flex-col gap-3 p-3">
		<!-- Signing key -->
		<div>
			<div class="flex items-center justify-between gap-2">
				<div class="flex min-w-0 items-center gap-2 text-sm font-semibold">
					<KeyRound size={14} />
					{m.settings_signing_key_label()}
				</div>
				<button
					type="button"
					class="btn btn-ghost btn-xs"
					disabled={keyBusy}
					onclick={createKey}
				>
					{hasKey ? m.settings_signing_key_replace() : m.converter_signing_key_create()}
				</button>
			</div>
			{#if hasKey}
				<p class="truncate font-mono text-xs opacity-70">
					{keyId ?? m.settings_signing_key_present()}
				</p>
			{:else}
				<p class="text-xs opacity-60">{m.settings_signing_key_none()}</p>
			{/if}
			{#if keyBusy}
				<p class="text-warning mt-1 flex items-center gap-1 text-xs">
					<span class="loading loading-spinner loading-xs"></span>
					{m.converter_signing()}
				</p>
			{:else if keyError}
				<p class="text-error mt-1 text-xs">{keyError}</p>
			{:else if hasKey}
				<p class="text-base-content/50 mt-0.5 text-xs">
					{m.settings_signing_key_replace_hint()}
				</p>
			{/if}
			{#if hasKey}
				<div class="mt-2">
					<button
						type="button"
						class="btn btn-ghost btn-xs gap-1"
						onclick={() => (showVerify = !showVerify)}
					>
						<ShieldCheck size={14} />
						{m.settings_signing_key_verify()}
					</button>
					{#if showVerify}
						<div class="bg-base-300/50 mt-1 flex flex-col gap-2 rounded-lg p-3">
							<p class="text-xs opacity-60">
								{m.settings_signing_key_verify_hint()}
							</p>
							<div class="flex items-center gap-2">
								<input
									type="text"
									class="input input-bordered input-sm flex-1"
									placeholder={m.settings_signing_key_verify_text_placeholder()}
									bind:value={verifyText}
									onkeydown={(e) =>
										e.key === "Enter" && verifyText.trim() && verifyKey()}
								/>
								<button
									type="button"
									class="btn btn-primary btn-sm"
									disabled={verifyBusy || !verifyText.trim()}
									onclick={verifyKey}
								>
									{#if verifyBusy}
										<span class="loading loading-spinner loading-xs"></span>
									{/if}
									{m.settings_signing_key_verify_sign()}
								</button>
							</div>
							{#if verifyError}
								<p class="text-error text-xs">{verifyError}</p>
							{/if}
							{#if verifySignature}
								<textarea
									class="textarea textarea-bordered h-32 w-full font-mono text-xs"
									readonly
									value={verifySignature}></textarea>
								<button
									type="button"
									class="btn btn-sm self-end"
									onclick={copySignature}
								>
									{m.settings_signing_key_verify_copy()}
								</button>
							{/if}
						</div>
					{/if}
				</div>
			{/if}
		</div>

		<!-- API key -->
		{#if publishingEnabled}
			<div>
				<div class="flex items-center justify-between gap-2">
					<div class="text-sm font-semibold">{m.settings_api_key_label()}</div>
					{#if hasApiKey && !editingApiKey}
						<div class="flex items-center gap-1">
							<button
								type="button"
								class="btn btn-ghost btn-xs"
								onclick={() => (editingApiKey = true)}
							>
								{m.settings_api_key_replace()}
							</button>
							<button
								type="button"
								class="btn btn-ghost btn-xs text-error"
								onclick={forgetApiKey}
							>
								{m.settings_api_key_remove()}
							</button>
						</div>
					{:else if hasApiKey}
						<button
							type="button"
							class="btn btn-ghost btn-xs"
							onclick={() => (editingApiKey = false)}
						>
							{m.common_cancel()}
						</button>
					{/if}
				</div>

				{#if hasApiKey && !editingApiKey}
					<p class="font-mono text-xs opacity-70">{maskedKey}</p>
				{:else}
					{#if !editingApiKey}
						<p class="text-xs opacity-60">{m.settings_api_key_none()}</p>
					{/if}
					<div class="mt-1 flex items-center gap-2">
						<input
							type="password"
							class="input input-bordered input-sm flex-1"
							placeholder={m.converter_api_key_placeholder()}
							bind:value={apiKey}
							onkeydown={(e) => e.key === "Enter" && apiKey.trim() && saveApiKey()}
						/>
						<button
							type="button"
							class="btn btn-primary btn-sm"
							disabled={apiBusy || !apiKey.trim()}
							onclick={saveApiKey}
						>
							{#if apiBusy}
								<span class="loading loading-spinner loading-xs"></span>
								{m.converter_logging_in()}
							{:else}
								{m.converter_login_btn()}
							{/if}
						</button>
					</div>
					{#if apiError}
						<p class="text-error mt-1 text-xs">{apiError}</p>
					{/if}
				{/if}
			</div>
		{/if}

		{#if !publishingEnabled}
			<p class="text-base-content/50 text-xs">{m.settings_publishing_disabled()}</p>
		{/if}
	</div>
</div>
