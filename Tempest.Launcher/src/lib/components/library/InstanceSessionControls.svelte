<script lang="ts">
	import { Play, Plus, Square, ChevronDown } from "@lucide/svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { stopGameSession } from "$lib/core/launch";
	import { createLaunchGameMutation } from "$lib/queries/core";
	import { launchingInstanceIds, processesList } from "$lib/stores/processes.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	let { instance, busy = false }: { instance: Instance; busy?: boolean } = $props();
	const launch = createLaunchGameMutation();
	const sessions = $derived(processesList.value.filter((p) => p.instance.id === instance.id));
	const disabled = $derived(
		busy ||
			launchingInstanceIds.value.includes(instance.id) ||
			sessions.some((p) => p.status === "stopping"),
	);
	async function additional(event: MouseEvent) {
		event.stopPropagation();
		try {
			await launch.mutateAsync(instance);
		} catch (error) {
			addToast({ title: "Additional launch failed", message: String(error), tone: "error" });
		}
	}
	async function stop(sessionId: string) {
		try {
			await stopGameSession(sessionId);
		} catch (error) {
			addToast({ title: "Could not stop session", message: String(error), tone: "error" });
		}
	}
</script>

{#if sessions.length}
	<button
		class="btn btn-sm btn-accent gap-2"
		type="button"
		title="Open new window"
		aria-label="Open new window"
		onclick={additional}
		{disabled}
	>
		{#if launch.isPending}<span class="loading loading-spinner loading-xs"></span>
		{:else}<span class="relative"
				><Play size={12} /><Plus size={8} class="absolute -right-1 -bottom-1" /></span
			>{/if}
		<span>Open new window</span>
	</button>
	<PopoverMenu>
		{#snippet trigger()}
			<span class="btn btn-xs" title="Manage running sessions"
				>{sessions.length} running<ChevronDown size={12} /></span
			>
		{/snippet}
		{#snippet children()}
			<li class="menu-title">Running sessions</li>
			{#each sessions as session (session.sessionId)}
				<PopoverMenuItem
					onclick={() => stop(session.sessionId)}
					disabled={session.status === "stopping"}
					title="Stop only this session"
				>
					<Square size={12} /><span
						>Session {session.sessionNumber}<span class="block text-xs opacity-60"
							>{session.status === "stopping"
								? "Stopping…"
								: new Date(session.startedAt).toLocaleTimeString([], {
										hour: "2-digit",
										minute: "2-digit",
									})}</span
						></span
					>
				</PopoverMenuItem>
			{/each}
		{/snippet}
	</PopoverMenu>
{/if}
