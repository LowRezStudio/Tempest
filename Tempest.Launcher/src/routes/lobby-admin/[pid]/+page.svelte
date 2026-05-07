<script lang="ts">
	import { Square, SquareTerminal } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import Header from "$lib/components/ui/Header.svelte";
	import { moveToLobby } from "$lib/core/lobby";
	import { m } from "$lib/paraglide/messages";
	import { createKillLobbyServerMutation } from "$lib/queries/lobby";
	import { lobbyServerProcessesList } from "$lib/stores/processes";

	let activeTab = $state<"logs">("logs");

	const process = $derived(
		$lobbyServerProcessesList.find((p) => String(p.child.pid) === page.params.pid!),
	);
	const killLobbyMutation = createKillLobbyServerMutation();

	const logs = $derived(process?.logs);
	const isKilling = $derived(killLobbyMutation.isPending);
	const returnCode = $derived(process?.returnCode);
	const isRunning = $derived($returnCode === null);
	const hasError = $derived($returnCode !== null && $returnCode !== 0);

	function handleStopOrClose() {
		if (!process) return;
		if (isRunning) {
			//stopping
			killLobbyMutation.mutate(process);
		} else {
			//closing
			lobbyServerProcessesList.set(
				lobbyServerProcessesList.get().filter((p) => p.child.pid !== process.child.pid),
			);
			killLobbyMutation.reset();
			goto("/servers");
		}
	}
	function join() {
		if (!process) return;
		moveToLobby(`http://127.0.0.1:${process.createOptions.port}`);
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header
		title={process?.createOptions.name || m.common_unknown()}
		tabs={[{ name: m.lobbyadmin_logs(), value: "logs" }]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
	>
		{#snippet icon()}
			<SquareTerminal size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			{#if hasError}
				{m.lobbyadmin_encountered_error()}
			{:else if !isRunning}
				{m.lobbyadmin_stopped()}
			{/if}
		{/snippet}
		{#snippet actions()}
			{#if isRunning}
				<button class="btn text-sm btn-accent" onclick={join}> {m.common_join()} </button>
			{/if}

			<button
				class="btn text-sm btn-error"
				disabled={isKilling}
				aria-busy={isKilling}
				onclick={handleStopOrClose}
			>
				{#if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
					{m.lobbyadmin_stopping()}
				{:else if isRunning}
					<Square size={16} />
					{m.common_stop()}
				{:else if !isRunning}
					{m.common_close()}
				{/if}
			</button>
		{/snippet}
	</Header>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		{#if activeTab === "logs"}
			<div class="px-4 py-6 overflow-y-auto">
				{#each $logs as log (log.id)}
					<div>
						{log.error ? `ERR: ${log.line}` : log.line}
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
