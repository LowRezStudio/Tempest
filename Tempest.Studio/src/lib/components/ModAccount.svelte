<!--
	Signing and publishing, as the CLI keeps them (shared with the launcher):
	the signing key (who signed shows next to each mod; create it once, and
	sign the website's verification text to prove it is yours) and the API key
	of the mod repository (log in, replace, forget).
-->
<script lang="ts">
	import { ExternalLink, KeyRound, KeySquare, ShieldCheck } from "@lucide/svelte";
	import { invoke } from "$lib/platform/api";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	let keyName = $state("");
	let showVerify = $state(false);
	let verifyText = $state("");
	let signature = $state("");
	let verifyError = $state("");
	let verifying = $state(false);
	let apiKey = $state("");
	let editingApi = $state(false);
	let apiError = $state("");
	let loggingIn = $state(false);

	const account = $derived(cooker.account);
	const repo = $derived(account?.repo.url ?? "https://mods.lowrezstudio.com");

	$effect(() => {
		void cooker.loadAccount();
	});

	const site = (anchor: string) =>
		invoke("shell:open-url", { url: `${repo}/settings#${anchor}` });

	async function createKey() {
		if (!keyName.trim()) return;
		if (await cooker.createKey(keyName)) keyName = "";
	}

	async function verify() {
		if (!verifyText.trim() || verifying) return;
		verifying = true;
		verifyError = "";
		signature = "";
		try {
			signature = await cooker.signText(verifyText.trim());
		} catch (error) {
			verifyError = error instanceof Error ? error.message : String(error);
		} finally {
			verifying = false;
		}
	}

	async function copySignature() {
		try {
			await navigator.clipboard.writeText(signature);
			toasts.push(
				"success",
				"Paste it on the website to verify your key.",
				"Signature copied",
			);
		} catch (error) {
			toasts.error(error, "Could not copy");
		}
	}

	async function login() {
		if (!apiKey.trim() || loggingIn) return;
		loggingIn = true;
		apiError = "";
		const error = await cooker.login(apiKey);
		loggingIn = false;
		if (error) {
			apiError = error;
			return;
		}
		apiKey = "";
		editingApi = false;
		toasts.push("success", `Signed in to ${repo.replace(/^https?:\/\//, "")}`, "API key saved");
	}
</script>

<div class="flex flex-col gap-3 text-xs">
	<!-- Signing key -->
	<div class="flex flex-col gap-1">
		<div class="flex items-center gap-2">
			<span class="font-semibold">Signing key</span>
			<span class="flex-1"></span>
			<button class="btn btn-ghost btn-xs" onclick={() => site("signing-keys")}>
				Manage on the website <ExternalLink size={11} />
			</button>
		</div>
		<p class="opacity-60">
			Mods are signed so players can tell they come from you. Mods with scripts or DLLs only
			install when signed.
		</p>
		{#if account?.signing.hasKey}
			<div class="bg-base-100 border-base-300 rounded border px-2 py-1.5">
				<div class="flex flex-wrap items-center gap-2">
					<KeyRound size={13} class="text-primary" />
					<span class="min-w-0 flex-1 truncate font-mono" title={account.signing.path}
						>{account.signing.keyId ?? "your key"}</span
					>
					<button
						class="btn btn-xs {showVerify ? 'btn-primary btn-soft' : 'btn-ghost'}"
						aria-expanded={showVerify}
						onclick={() => (showVerify = !showVerify)}
						><ShieldCheck size={12} /> Verify on the website</button
					>
				</div>
				{#if showVerify}
					<div class="border-base-300 mt-2 flex flex-col gap-2 border-t pt-2">
						<p class="opacity-70">
							Copy the text the website shows under Signing keys, sign it here, then
							paste the signature back on the website.
						</p>
						<form
							class="flex gap-1"
							onsubmit={(e) => {
								e.preventDefault();
								void verify();
							}}
						>
							<input
								class="input input-xs min-w-0 flex-1"
								placeholder="text to sign"
								bind:value={verifyText}
							/>
							<button
								class="btn btn-primary btn-xs"
								disabled={verifying || !verifyText.trim()}
								>{#if verifying}<span class="loading loading-xs loading-spinner"
									></span>{/if}
								Sign</button
							>
						</form>
						{#if verifyError}<p class="text-error">{verifyError}</p>{/if}
						{#if signature}
							<textarea
								class="textarea textarea-xs h-28 w-full font-mono"
								readonly
								value={signature}></textarea>
							<button class="btn btn-xs self-end" onclick={copySignature}>Copy</button
							>
						{/if}
					</div>
				{/if}
			</div>
		{:else}
			<form
				class="flex flex-wrap items-center gap-1"
				onsubmit={(e) => {
					e.preventDefault();
					void createKey();
				}}
			>
				<input
					class="input input-xs min-w-0 flex-1"
					placeholder="your name or handle, shown next to your signature"
					bind:value={keyName}
				/>
				<button class="btn btn-primary btn-xs" disabled={!keyName.trim() || !!cooker.busy}
					><KeyRound size={12} /> Create my key</button
				>
			</form>
			<p class="opacity-60">
				The key is kept on this computer (shared with the launcher). Keep a copy of it: a
				new key is a new identity.
			</p>
		{/if}
	</div>

	<!-- API key -->
	<div class="flex flex-col gap-1">
		<div class="flex items-center gap-2">
			<span class="font-semibold">Mod repository</span>
			<span class="truncate opacity-60">{repo.replace(/^https?:\/\//, "")}</span>
			<span class="flex-1"></span>
			<button class="btn btn-ghost btn-xs" onclick={() => site("api-keys")}>
				Get an API key <ExternalLink size={11} />
			</button>
		</div>
		{#if account?.repo.hasApiKey && !editingApi}
			<div
				class="bg-base-100 border-base-300 flex items-center gap-2 rounded border px-2 py-1.5"
			>
				<KeySquare size={13} class="text-primary" />
				<span class="min-w-0 flex-1 truncate font-mono">{account.repo.masked}</span>
				<button class="btn btn-ghost btn-xs" onclick={() => (editingApi = true)}
					>Replace</button
				>
				<button
					class="btn btn-ghost btn-xs text-error"
					onclick={() => cooker.forgetApiKey()}>Forget</button
				>
			</div>
		{:else}
			<form
				class="flex gap-1"
				onsubmit={(e) => {
					e.preventDefault();
					void login();
				}}
			>
				<input
					type="password"
					class="input input-xs min-w-0 flex-1 font-mono"
					placeholder="API key (tm_live_…)"
					aria-label="API key"
					bind:value={apiKey}
				/>
				{#if editingApi}
					<button
						type="button"
						class="btn btn-ghost btn-xs"
						onclick={() => (editingApi = false)}>Cancel</button
					>
				{/if}
				<button class="btn btn-primary btn-xs" disabled={loggingIn || !apiKey.trim()}>
					{#if loggingIn}<span class="loading loading-xs loading-spinner"></span>{/if}
					Log in
				</button>
			</form>
			{#if apiError}<p class="text-error">{apiError}</p>{/if}
			<p class="opacity-60">
				Needed to publish. Stored on this computer, shared with the launcher.
			</p>
		{/if}
	</div>
</div>
