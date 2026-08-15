<script lang="ts">
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import { Square, SquareTerminal, Globe } from "@lucide/svelte";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import GhosttyTerminal from "$lib/components/ui/GhosttyTerminal.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { moveToLobby } from "$lib/core/lobby.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createKillLobbyServerMutation } from "$lib/queries/lobby";
	import { lobbyServerProcessesList } from "$lib/stores/processes.svelte";
	import type { ProcessLog } from "$lib/types/process";

	let activeTab = $state<"logs">("logs");

	const process = $derived(
		lobbyServerProcessesList.value.find((p) => String(p.child.pid) === page.params.pid!),
	);
	const killLobbyMutation = createKillLobbyServerMutation();

	const logsList = $derived(process?.logs ? process.logs.value : []);
	const isKilling = $derived(killLobbyMutation.isPending);
	const returnCode = $derived(process?.returnCode);
	const isRunning = $derived(returnCode ? returnCode.value === null : false);
	const hasError = $derived(
		returnCode ? returnCode.value !== null && returnCode.value !== 0 : false,
	);

	// After pressing Stop, close the page once the server has actually exited.
	let autoCloseAfterStop = $state(false);

	// True from the moment Stop is pressed until the process has truly exited,
	// so the button can show progress for the whole shutdown (not just the RPC).
	const isStopping = $derived(autoCloseAfterStop && isRunning);

	$effect(() => {
		if (!autoCloseAfterStop || isRunning) return;
		if (!process) return;
		lobbyServerProcessesList.value = lobbyServerProcessesList.value.filter(
			(p) => p.child.pid !== process.child.pid,
		);
		killLobbyMutation.reset();
		goto("/servers");
	});

	function handleStopOrClose() {
		if (!process) return;
		if (isRunning) {
			//stopping
			autoCloseAfterStop = true;
			killLobbyMutation.mutate(process);
		} else {
			//closing
			lobbyServerProcessesList.value = lobbyServerProcessesList.value.filter(
				(p) => p.child.pid !== process.child.pid,
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

<div class="bg-base-100 flex h-full flex-col">
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
			<button class="btn" onclick={() => openUrl("https://myip.wtf/")}>
				<Globe size={16} />
				{m.lobbyadmin_whats_my_ip()}
			</button>

			{#if isRunning && !isStopping}
				<button class="btn btn-accent text-sm" onclick={join}>
					{m.common_join()}
				</button>
			{/if}

			<button
				class="btn btn-error text-sm"
				disabled={isStopping || isKilling}
				aria-busy={isStopping || isKilling}
				onclick={handleStopOrClose}
			>
				{#if isStopping || isKilling}
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

	<div class="bg-base-100 flex-1 overflow-hidden">
		{#if activeTab === "logs"}
			<GhosttyTerminal logs={logsList} child={process?.child} />
		{/if}
	</div>
</div>
