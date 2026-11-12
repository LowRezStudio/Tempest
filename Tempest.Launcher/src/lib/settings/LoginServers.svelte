<script lang="ts">
	import { Plus, Trash2 } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { loginServers } from "$lib/stores/login-servers.svelte";
	import type { LoginServer } from "$lib/types/login-server";

	let name = $state("");
	let address = $state("");
	let adding = $state(false);
	let error = $state("");
	let addOpen = $state(false);

	const servers = $derived(loginServers.value);

	function resetAddForm() {
		name = "";
		address = "";
		error = "";
	}

	function closeAddDialog() {
		addOpen = false;
		resetAddForm();
	}

	async function addLoginServer() {
		if (adding) return;
		adding = true;
		error = "";
		try {
			const result = await invoke("login-server:discover", { url: address });
			if (!result.ok) {
				error = m.settings_login_servers_discover_failed({ error: result.message });
				return;
			}
			const discovery = result.loginServer;
			const existing = loginServers.value.find(
				(server) => server.baseUrl === discovery.baseUrl,
			);
			const server: LoginServer = {
				...discovery,
				id: existing?.id ?? crypto.randomUUID(),
				name: name.trim() || existing?.name || new URL(discovery.baseUrl).hostname,
			};
			loginServers.value = existing
				? loginServers.value.map((item) => (item.id === existing.id ? server : item))
				: [...loginServers.value, server];
			closeAddDialog();
		} catch (error_) {
			console.error("Login server discovery failed unexpectedly:", error_);
			error = m.settings_login_servers_unexpected_failure();
		} finally {
			adding = false;
		}
	}

	function removeLoginServer(id: string) {
		loginServers.value = loginServers.value.filter((server) => server.id !== id);
	}
</script>

<div class="flex flex-col gap-3">
	{#if servers.length > 0}
		<div class="flex flex-col gap-2">
			{#each servers as server (server.id)}
				<div
					class="bg-base-100 rounded-field border-base-300 flex items-center gap-3 border px-3 py-2"
				>
					<div class="min-w-0 flex-1">
						<p class="truncate text-sm font-medium">{server.name}</p>
						<p class="text-base-content/60 truncate font-mono text-xs">
							{server.baseUrl}
						</p>
						<p class="text-base-content/60 truncate font-mono text-xs">
							{server.addresses.join(", ")}
						</p>
					</div>
					<button
						type="button"
						class="btn btn-ghost btn-xs text-error shrink-0"
						aria-label={m.settings_login_servers_remove_name({ name: server.name })}
						onclick={() => removeLoginServer(server.id)}
					>
						<Trash2 size={12} />
						{m.settings_login_servers_remove()}
					</button>
				</div>
			{/each}
		</div>
	{:else}
		<p class="text-base-content/60 text-xs">{m.settings_login_servers_empty()}</p>
	{/if}

	<div class="flex justify-start">
		<button type="button" class="btn btn-sm" onclick={() => (addOpen = true)}>
			<Plus size={14} />
			{m.settings_login_servers_add()}
		</button>
	</div>
</div>

<Modal
	bind:open={addOpen}
	title={m.settings_login_servers_add_title()}
	icon={Plus}
	description={m.settings_login_servers_add_description()}
	class="max-w-lg"
	dismissible={!adding}
	closeOnOutsideClick={!adding}
	onsubmit={addLoginServer}
	onclose={resetAddForm}
>
	<div class="flex flex-col gap-3">
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.settings_login_servers_name()}</span>
			<input
				type="text"
				class="input input-sm w-full"
				placeholder={m.settings_login_servers_name_placeholder()}
				bind:value={name}
			/>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.settings_login_servers_address()}</span>
			<input
				type="text"
				class="input input-sm w-full font-mono"
				placeholder={m.settings_login_servers_address_placeholder()}
				spellcheck="false"
				autocapitalize="off"
				required
				bind:value={address}
			/>
		</label>
		{#if error}
			<p role="alert" class="text-error text-xs break-words">{error}</p>
		{/if}
	</div>

	{#snippet actions()}
		<button
			type="button"
			class="btn btn-ghost btn-sm"
			disabled={adding}
			onclick={closeAddDialog}
		>
			{m.common_cancel()}
		</button>
		<button type="submit" class="btn btn-primary btn-sm" disabled={adding || !address.trim()}>
			{#if adding}
				<span class="loading loading-spinner loading-xs"></span>
			{:else}
				<Plus size={14} />
			{/if}
			{m.settings_login_servers_add()}
		</button>
	{/snippet}
</Modal>
