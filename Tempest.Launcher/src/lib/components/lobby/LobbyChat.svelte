<script lang="ts">
	import { Globe, Users, X } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { tick } from "svelte";
	import type { ChatMessage } from "$lib/lobby/stores.svelte";

	interface Props {
		messages: readonly ChatMessage[];
		disabled: boolean;
		handleSendChatMessage: (message: string, channel: string) => void;
	}

	let { messages, disabled, handleSendChatMessage }: Props = $props();

	const channelLabels = {
		global: m.lobby_channel_global,
		team: m.lobby_channel_team,
	} as const;

	let channel = $state<keyof typeof channelLabels>("global");
	let filteredMessages = $derived(messages.filter((m) => m.channel === channel));

	let unreadGlobal = $state(false);
	let unreadTeam = $state(false);
	let prevGlobalCount = 0;
	let prevTeamCount = 0;

	$effect(() => {
		const globalCount = messages.filter((m) => m.channel === "global").length;
		const teamCount = messages.filter((m) => m.channel === "team").length;
		if (globalCount > prevGlobalCount && !(open && channel === "global")) {
			unreadGlobal = true;
		}
		if (teamCount > prevTeamCount && !(open && channel === "team")) {
			unreadTeam = true;
		}
		prevGlobalCount = globalCount;
		prevTeamCount = teamCount;
	});

	$effect(() => {
		void filteredMessages.length;
		if (!chatContainer) return;
		const nearBottom =
			chatContainer.scrollTop + chatContainer.clientHeight >= chatContainer.scrollHeight - 40;
		if (!nearBottom) return;

		tick().then(() => {
			if (!chatContainer) return;
			chatContainer.scrollTop = chatContainer.scrollHeight;
		});
	});

	let chatContainer = $state<HTMLDivElement>();
	let chatInput = $state<HTMLInputElement>();
	let open = $state<boolean>(false);
	let chatboxText = $state<string>("");

	function openChannel(ch: keyof typeof channelLabels) {
		channel = ch;
		open = true;
		if (ch === "global") unreadGlobal = false;
		else unreadTeam = false;
		tick().then(() => {
			chatInput?.focus();
			if (!chatContainer) return;
			chatContainer.scrollTop = chatContainer.scrollHeight;
		});
	}
</script>

{#if open}
	<div
		class="absolute bottom-8 left-8 z-20 w-96 bg-base-200/95 backdrop-blur-xs rounded-lg shadow-xl flex flex-col h-[300px]"
	>
		<div class="px-2 pt-2 border-b border-base-300 flex items-center justify-between">
			<div role="tablist" class="tabs tabs-border">
				<button
					role="tab"
					class="tab"
					class:tab-active={channel === "global"}
					onclick={() => { channel = "global"; unreadGlobal = false; chatInput?.focus(); }}
				>
					{m.lobby_channel_global()}
				</button>
				<button
					role="tab"
					class="tab"
					class:tab-active={channel === "team"}
					onclick={() => { channel = "team"; unreadTeam = false; chatInput?.focus(); }}
				>
					{m.lobby_channel_team()}
				</button>
			</div>
			<button
				class="btn btn-ghost btn-sm btn-square"
				onclick={() => { open = false; if (channel === "global") unreadGlobal = false; else unreadTeam = false; }}
				aria-label={m.lobby_close_chat()}
			>
				<X size={14} />
			</button>
		</div>

		<div class="flex-1 overflow-y-auto p-3 min-h-0" bind:this={chatContainer}>
			{#if filteredMessages.length === 0}
				<p class="text-sm opacity-50 text-center py-2">{m.lobby_no_messages()}</p>
			{:else}
				<div class="flex flex-col gap-1.5">
					{#each filteredMessages as msg (msg.sentAt)}
						<div class="text-sm">
							<span class="font-semibold">{msg.username}</span>
							<span class="opacity-70">: {msg.content}</span>
						</div>
					{/each}
				</div>
			{/if}
		</div>

		<div class="p-2 border-t border-base-300">
			<input
				type="text"
				class="input input-bordered input-sm w-full"
				bind:this={chatInput}
				{disabled}
				placeholder={m.lobby_type_message()}
				maxlength={100}
				autocomplete="off"
				bind:value={chatboxText}
				onkeydown={(e) => {
					if (e.key === "Enter") {
						handleSendChatMessage(chatboxText, channel);
						chatboxText = "";
					}
				}}
			/>
		</div>
	</div>
{:else}
	<div class="absolute bottom-8 left-8 z-20 flex flex-row gap-2">
		<button
			class="btn btn-sm shadow-none justify-start"
			class:btn-accent={unreadGlobal}
			onclick={() => openChannel("global")}
		>
			<Globe size={16} />
			{m.lobby_channel_global()}
		</button>
		<button
			class="btn btn-sm shadow-none justify-start"
			class:btn-accent={unreadTeam}
			onclick={() => openChannel("team")}
		>
			<Users size={16} />
			{m.lobby_channel_team()}
		</button>
	</div>
{/if}
