<script lang="ts">
	import { MessageSquare, X } from "@lucide/svelte";
	import { tick } from "svelte";
	import { m } from "$lib/paraglide/messages";
	import type { ChatMessage } from "$lib/lobby/stores.svelte";

	interface Props {
		messages: readonly ChatMessage[];
		disabled: boolean;
		handleSendChatMessage: (message: string, channel: string) => void;
		children?: import("svelte").Snippet;
	}

	let { messages, disabled, handleSendChatMessage, children }: Props = $props();

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
		class="bg-base-200/95 absolute bottom-8 left-8 z-20 flex h-[300px] w-96 flex-col rounded-lg shadow-xl backdrop-blur-xs"
	>
		<div class="border-base-300 flex items-center justify-between border-b px-2 pt-2">
			<span class="px-2 text-sm font-semibold">{m.lobby_channel_global()}</span>
			<button
				class="btn btn-ghost btn-sm btn-square"
				onclick={closeChat}
				aria-label={m.lobby_close_chat()}
			>
				<X size={14} />
			</button>
		</div>

		<div class="min-h-0 flex-1 overflow-y-auto p-3" bind:this={chatContainer}>
			{#if messages.filter((msg) => msg.channel === "global").length === 0}
				<p class="py-2 text-center text-sm opacity-50">{m.lobby_no_messages()}</p>
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

		<div class="border-base-300 border-t p-2">
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
	<div class="absolute bottom-8 left-8 z-20 flex flex-row items-end gap-2">
		<button
			class="btn btn-sm justify-start shadow-none"
			class:btn-accent={unread}
			onclick={openChat}
		>
			<MessageSquare size={16} />
			{m.lobby_channel_global()}
		</button>
		{@render children?.()}
	</div>
{/if}
