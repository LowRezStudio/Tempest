<script lang="ts">
	import { Bug, Loader2 } from "@lucide/svelte";
	import { getAppVersion } from "$lib/core/appVersion";
	import { closeCrashDialog, crashDialog } from "$lib/core/crash.svelte";
	import { listMods } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { arch, invoke, openUrl, type, version } from "$lib/platform/api";
	import { processLogs } from "$lib/stores/processes.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import Modal from "./Modal.svelte";
	import type { CrashDetails } from "$ipc/contract";

	const DISCORD_URL = "https://discord.gg/yZDuxAk4d9";
	const LAUNCHER_LOG_LINES = 500;

	let note = $state("");
	let includeDump = $state(false);
	let busy = $state<"send" | "save" | null>(null);
	let sendError = $state<string | null>(null);

	const crash = $derived(crashDialog.value.crash);
	const summary = $derived(crash ? [...crash.message, ...crash.stack].join("\n") : "");

	const formatSize = (bytes: number) =>
		bytes < 1024 * 1024
			? `${Math.max(1, Math.round(bytes / 1024))} KB`
			: `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

	const close = () => {
		closeCrashDialog();
		note = "";
		includeDump = false;
		sendError = null;
	};

	async function details(): Promise<CrashDetails> {
		const instance = crashDialog.value.instance;
		const mods = instance ? await listMods(instance.path).catch(() => []) : [];
		return {
			game: instance
				? [instance.version, instance.label, instance.launchOptions.platform ?? "Win64"]
						.filter(Boolean)
						.join(" ")
				: "unknown",
			launcher: await getAppVersion(),
			os: `${type()} ${version()} ${arch()}`.trim(),
			mods: mods
				.filter((mod) => mod.Enabled)
				.map((mod) => `${mod.Name} ${mod.Version}`.trim()),
			note: note.trim(),
			launcherLog: processLogs.value
				.filter((log) => log.source === "launch")
				.slice(-LAUNCHER_LOG_LINES)
				.map((log) => log.line)
				.join("\n"),
		};
	}

	async function send() {
		if (!crash) return;
		busy = "send";
		sendError = null;
		try {
			const result = await invoke("crash:send", {
				id: crash.id,
				details: await details(),
				includeDump,
			});
			if (result.status === "sent") {
				addToast({ message: m.crash_sent(), tone: "success" });
				close();
			} else if (result.status === "rate-limited") {
				sendError = m.crash_send_rate_limited();
			} else if (result.status === "too-large") {
				sendError = m.crash_send_too_large();
			} else {
				sendError = m.crash_send_failed({ error: result.error ?? "" });
			}
		} catch (error) {
			sendError = m.crash_send_failed({ error: String(error) });
		} finally {
			busy = null;
		}
	}

	async function save() {
		if (!crash) return;
		busy = "save";
		try {
			const saved = await invoke("crash:save", {
				id: crash.id,
				details: await details(),
				includeDump,
			});
			if (saved) addToast({ title: m.crash_saved(), message: saved, tone: "success" });
		} catch (error) {
			addToast({ message: String(error), tone: "error" });
		} finally {
			busy = null;
		}
	}
</script>

<Modal
	bind:open={crashDialog.value.open}
	title={m.crash_title()}
	description={m.crash_desc()}
	icon={Bug}
	class="max-w-xl"
	dismissible={!busy}
	closeOnOutsideClick={false}
	onclose={close}
>
	{#if crash}
		<div class="space-y-4 text-sm">
			{#if summary}
				<pre
					class="bg-base-200 rounded-box max-h-32 overflow-auto p-3 font-mono text-xs">{summary}</pre>
			{/if}

			<label class="block space-y-1">
				<span class="block font-medium">{m.crash_note_label()}</span>
				<textarea
					class="textarea w-full"
					rows="2"
					maxlength="900"
					placeholder={m.crash_note_placeholder()}
					bind:value={note}></textarea>
			</label>

			<div class="space-y-1">
				<p class="font-medium">{m.crash_included()}</p>
				<ul class="space-y-0.5 opacity-80">
					<li>{m.crash_file_report()}</li>
					{#each crash.files as file (file.name)}
						<li class="font-mono text-xs">{file.name} ({formatSize(file.size)})</li>
					{/each}
					<li>{m.crash_file_launcher()}</li>
				</ul>
				{#if crash.dumpSize !== null}
					<label class="flex items-start gap-2 pt-2">
						<input
							type="checkbox"
							class="checkbox checkbox-sm"
							bind:checked={includeDump}
						/>
						<span>
							{m.crash_include_dump({ size: formatSize(crash.dumpSize) })}
							<span class="block opacity-60">{m.crash_dump_hint()}</span>
						</span>
					</label>
				{/if}
			</div>

			<p class="opacity-60">{m.crash_privacy()}</p>

			{#if sendError}
				<p class="text-error" role="alert">
					{sendError}
					<button class="link" type="button" onclick={() => void openUrl(DISCORD_URL)}>
						{m.crash_open_discord()}
					</button>
				</p>
			{/if}
		</div>
	{/if}

	{#snippet actions()}
		<button class="btn btn-ghost" type="button" disabled={!!busy} onclick={close}>
			{m.crash_not_now()}
		</button>
		<button class="btn" type="button" disabled={!!busy} onclick={save}>
			{#if busy === "save"}
				<Loader2 size={16} class="animate-spin" />
			{/if}
			{m.crash_save()}
		</button>
		<button class="btn btn-primary" type="button" disabled={!!busy} onclick={send}>
			{#if busy === "send"}
				<Loader2 size={16} class="animate-spin" />
				{m.crash_sending()}
			{:else}
				{m.crash_report()}
			{/if}
		</button>
	{/snippet}
</Modal>
