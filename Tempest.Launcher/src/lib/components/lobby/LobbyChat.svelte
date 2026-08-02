<script lang="ts">
	import { MessageSquare, X } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { tick } from "svelte";
	import type { ChatMessage } from "$lib/lobby/stores.svelte";

	interface Props {
		messages: readonly ChatMessage[];
		disabled: boolean;
		handleSendChatMessage: (message: string, channel: string) => void;
	}

	let { messages, disabled, handleSendChatMessage }: Props = $props();

	let unread = $state(false);
	let prevCount = 0;

	$effect(() => {
		const count = messages.filter((msg) => msg.channel === "global").length;
		if (count > prevCount && !(open && true)) {
			unread = true;
		}
		prevCount = count;
	});

	$effect(() => {
		void messages.length;
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

	function openChat() {
		open = true;
		unread = false;
		tick().then(() => {
			chatInput?.focus();
			if (!chatContainer) return;
			chatContainer.scrollTop = chatContainer.scrollHeight;
		});
	}

	function closeChat() {
		open = false;
		unread = false;
	}
</script>

{#if open}
	<div
		class="absolute bottom-8 left-8 z-20 w-96 bg-base-200/95 backdrop-blur-xs rounded-lg shadow-xl flex flex-col h-[300px]"
	>
		<div class="px-2 pt-2 border-b border-base-300 flex items-center justify-between">
			<span class="text-sm font-semibold px-2">{m.lobby_channel_global()}</span>
			<button
				class="btn btn-ghost btn-sm btn-square"
				onclick={closeChat}
				aria-label={m.lobby_close_chat()}
			>
				<X size={14} />
			</button>
		</div>

		<div class="flex-1 overflow-y-auto p-3 min-h-0" bind:this={chatContainer}>
			{#if messages.filter((msg) => msg.channel === "global").length === 0}
				<p class="text-sm opacity-50 text-center py-2">{m.lobby_no_messages()}</p>
			{:else}
				<div class="flex flex-col gap-1.5">
					{#each messages.filter((msg) => msg.channel === "global") as msg (msg.sentAt)}
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
						handleSendChatMessage(chatboxText, "global");
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
			class:btn-accent={unread}
			onclick={openChat}
		>
			<MessageSquare size={16} />
			{m.lobby_channel_global()}
		</button>
	</div>
{/if}
