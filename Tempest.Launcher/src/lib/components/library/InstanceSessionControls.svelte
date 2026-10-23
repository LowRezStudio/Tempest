<script lang="ts">
	import { Layers, Plus, Square } from "@lucide/svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { stopGameWindow } from "$lib/core/launch";
	import { launchPresets } from "$lib/core/presets";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchGameMutation } from "$lib/queries/core";
	import { processesList } from "$lib/stores/processes.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";
	import type { Process } from "$lib/types/process";

	interface Props {
		instance: Instance;
		busy?: boolean;
		/** Matches the trigger to the buttons it sits next to. */
		variant?: "card" | "header" | "home";
	}

	let { instance, busy = false, variant = "card" }: Props = $props();

	const variants = {
		card: { trigger: "btn btn-sm btn-ghost gap-1 px-2", icon: 14, count: "text-xs" },
		header: { trigger: "btn gap-1.5", icon: 16, count: "text-sm" },
		home: { trigger: "btn btn-lg join-item min-h-14 gap-1.5", icon: 20, count: "text-sm" },
	};
	let style = $derived(variants[variant]);

	const launchMutation = createLaunchGameMutation();

	let windows = $derived(processesList.value.filter((p) => p.instance.id === instance.id));
	let disabled = $derived(busy || launchMutation.isPending);

	const formatTime = (timestamp: number) =>
		new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

	// Name each window after the preset it was launched with, if any.
	const windowLabel = (gameWindow: Process): string => {
		const preset = launchPresets(gameWindow.instance).find(
			(preset) => preset.id === gameWindow.instance.launchOptions.presetId,
		);
		return preset
			? `${preset.title || m.instance_preset_untitled()} ${gameWindow.sessionNumber}`
			: m.sessions_window({ number: gameWindow.sessionNumber });
	};

	async function openAnother() {
		try {
			await launchMutation.mutateAsync(instance);
		} catch (error) {
			addToast({ title: m.sessions_launch_failed(), message: String(error), tone: "error" });
		}
	}
</script>

{#if windows.length}
	<PopoverMenu triggerClass={style.trigger} triggerLabel={m.sessions_manage()} menuClass="w-80">
		{#snippet trigger()}
			{#if launchMutation.isPending}
				<span class="loading loading-spinner loading-xs"></span>
			{:else}
				<Layers size={style.icon} />
			{/if}
			<span class="{style.count} font-semibold tabular-nums">{windows.length}</span>
		{/snippet}
		{#snippet children()}
			<li class="menu-title">{m.sessions_title()}</li>
			{#each windows as gameWindow (gameWindow.child.pid)}
				<PopoverMenuItem
					onclick={() => stopGameWindow(gameWindow)}
					disabled={gameWindow.status === "stopping"}
					title={m.common_stop()}
				>
					{#if gameWindow.status === "stopping"}
						<span class="loading loading-spinner loading-xs"></span>
					{:else}
						<Square size={16} />
					{/if}
					<span class="w-40 truncate" title={windowLabel(gameWindow)}>
						{windowLabel(gameWindow)}
					</span>
					<span class="max-w-20 truncate text-xs tabular-nums opacity-60">
						{gameWindow.status === "stopping"
							? m.common_stopping_label()
							: formatTime(gameWindow.startedAt)}
					</span>
				</PopoverMenuItem>
			{/each}
			<li role="separator" class="border-base-content/10 my-1 border-t"></li>
			<PopoverMenuItem onclick={openAnother} {disabled}>
				<Plus size={16} />
				{m.sessions_open_another()}
			</PopoverMenuItem>
		{/snippet}
	</PopoverMenu>
{/if}
