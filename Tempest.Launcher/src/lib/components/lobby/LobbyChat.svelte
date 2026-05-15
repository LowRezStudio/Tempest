<script lang="ts">
	import { MessageCircle, X } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { tick } from "svelte";
	import type { ChatMessage } from "$lib/lobby/stores";

	interface Props {
		messages: readonly ChatMessage[];
		disabled: boolean;
		handleSendChatMessage: (message: string) => void;
	}

	let { messages, disabled, handleSendChatMessage }: Props = $props();

	//automatically scroll to bottom if near the bottom when new messages come (40px thresshold)
	$effect(() => {
		void messages.length; // dependency
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
	let open = $state<boolean>(false);
	let chatboxText = $state<string>("");
</script>

{#if open}
	<div
		class="absolute bottom-4 left-4 z-20 w-96 bg-base-200/95 backdrop-blur-xs rounded-lg shadow-xl flex flex-col max-h-96"
	>
		<div class="px-3 py-2 border-b border-base-300 flex items-center justify-between">
			<div class="flex items-center gap-2">
				<MessageCircle size={16} />
				<span class="font-semibold text-sm">{m.lobby_team_chat()}</span>
			</div>
			<button
				class="btn btn-ghost btn-sm btn-square"
				onclick={() => (open = false)}
				aria-label={m.lobby_close_chat()}
			>
				<X size={14} />
			</button>
		</div>

		<div class="flex-1 overflow-y-auto p-3 min-h-0" bind:this={chatContainer}>
			{#if messages.length === 0}
				<p class="text-sm opacity-50 text-center py-2">{m.lobby_no_messages()}</p>
			{:else}
				<div class="flex flex-col gap-1.5">
					{#each messages as msg (msg.sentAt)}
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
				{disabled}
				placeholder={m.lobby_type_message()}
				maxlength={100}
				autocomplete="off"
				bind:value={chatboxText}
				onkeydown={(e) => {
					if (e.key === "Enter") {
						handleSendChatMessage(chatboxText);
						chatboxText = "";
					}
				}}
			/>
		</div>
	</div>
{:else}
	<button
		class="btn btn-accent absolute bottom-4 left-4 z-20"
		onclick={() => {
			open = true;
			tick().then(() => {
				if (!chatContainer) return;
				chatContainer.scrollTop = chatContainer.scrollHeight;
			});
		}}
	>
		<MessageCircle size={18} />
		{m.lobby_chat()}
	</button>
{/if}
