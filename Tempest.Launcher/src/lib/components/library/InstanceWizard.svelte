<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		Check,
		ChevronDown,
		CloudDownload,
		Copy,
		FolderOpen,
		Info,
		Plus,
		Terminal,
	} from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import versions from "$lib/data/versions.json";
	import { m } from "$lib/paraglide/messages";
	import { open as openDialog, join, platform } from "$lib/platform/api";
	import { RIGBY_BASE_URL, RIGBY_MANIFEST_URL_TEMPLATE } from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { addInstance } from "$lib/stores/instance.svelte";
	import { defaultInstancePath } from "$lib/stores/settings.svelte";
	import { instanceWizardTab } from "$lib/stores/ui.svelte";
	import { downloadablePatches } from "$lib/utils/versions";
	import {
		argsFor,
		findExistingInstance,
		FolderImport,
		freeLabel,
		newInstance,
		type LoginMethod,
	} from "./folder-import.svelte";
	import FolderImportPanel from "./FolderImportPanel.svelte";
	import LoginMethodPicker from "./LoginMethodPicker.svelte";
	import PatchPicker from "./PatchPicker.svelte";

	interface Props {
		open?: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	type Mode = "download" | "folder";

	const modes = [
		{
			mode: "download",
			Icon: CloudDownload,
			title: m.library_download_patch,
			hint: m.library_download_patch_hint,
		},
		{
			mode: "folder",
			Icon: FolderOpen,
			title: m.library_import_folder,
			hint: m.library_import_folder_hint,
		},
	] as const;

	let mode = $state<Mode>("download");
	let name = $state("");
	let downloadId = $state("");
	let installFolder = $state("");
	let installTarget = $state<{ path: string; copy: boolean } | null>(null);
	let loginMethod = $state<LoginMethod>();
	let copyStatus = $state<"idle" | "copied" | "failed">("idle");

	const importer = new FolderImport(
		() => open && mode === "folder",
		() => name,
	);

	const downloadPatch = $derived(versions.find((v) => v.id === downloadId));
	const patch = $derived(mode === "download" ? downloadPatch : importer.patch);

	const appId = $derived(patch?.appId ?? 444090);
	const isPts = $derived(
		(appId !== 444090 && appId !== 44090) || !!patch?.name?.toLowerCase().includes("pts"),
	);
	const depotId = $derived(isPts ? 596351 : 444091);

	// The import side asks for its own sign-in.
	const needsLogin = $derived(mode === "download" && downloadPatch?.version === "8.1");

	const ready = $derived(
		mode === "download" ? !!downloadPatch && (!needsLogin || !!loginMethod) : importer.ready,
	);

	const depotDownloaderCommand = $derived.by(() => {
		if (!downloadPatch) return "";
		const binary = platform() === "windows" ? ".\\DepotDownloader.exe" : "./DepotDownloader";
		const dir = installTarget?.path || "<install-path>";
		return `${binary} -app ${appId} -depot ${depotId} -manifest ${downloadPatch.id} -os windows -dir "${dir}" -qr -remember-password`;
	});

	// A patch already in the library gets another copy beside it: "8.1 (2)", then "8.1 (3)".
	async function resolveTarget(version: string) {
		const base = installFolder.trim() || defaultInstancePath.value;
		for (let n = 1; ; n++) {
			const folder = n === 1 ? version : `${version} (${n})`;
			const path = base ? await join(base, folder) : `/instances/${folder}`;
			if (!findExistingInstance(path)) return { path, copy: n > 1 };
		}
	}

	$effect(() => {
		const version = downloadPatch?.version;
		if (!version) {
			installTarget = null;
			return;
		}
		let stale = false;
		void resolveTarget(version).then((target) => {
			if (!stale) installTarget = target;
		});
		return () => {
			stale = true;
		};
	});

	async function browseInstallFolder() {
		const path = await openDialog({
			directory: true,
			title: m.wizard_select_install_folder(),
			defaultPath: installFolder || defaultInstancePath.value || undefined,
		});
		if (path) installFolder = path;
	}

	async function handleCreate() {
		if (!ready) return;

		if (mode === "download") {
			const target = downloadPatch!;
			const { path } = await resolveTarget(target.version);
			addInstance(
				newInstance(
					{
						label: name.trim() || freeLabel(target.name || target.version),
						version: target.version,
						manifestId: target.id,
						appId,
						path,
						state: { type: "downloading" },
					},
					argsFor(target.version, loginMethod),
				),
			);
			restoreQueue.add({
				manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", target.id)],
				outDir: path,
				baseUrl: RIGBY_BASE_URL,
			});
			open = false;
			void goto("/downloads");
			return;
		}

		importer.import();
		open = false;
	}

	$effect(() => {
		if (open) {
			mode = instanceWizardTab.value;
		} else {
			instanceWizardTab.value = "download";
			mode = "download";
			name = "";
			downloadId = "";
			installFolder = "";
			loginMethod = undefined;
			importer.reset();
		}
	});

	async function handleCopyCommand() {
		if (!depotDownloaderCommand) return;
		try {
			await navigator.clipboard.writeText(depotDownloaderCommand);
			copyStatus = "copied";
		} catch (error) {
			console.error("Failed to copy command:", error);
			copyStatus = "failed";
		}
		setTimeout(() => {
			copyStatus = "idle";
		}, 2000);
	}
</script>

<Modal
	bind:open
	title={m.wizard_title()}
	icon={Plus}
	description={m.wizard_description()}
	class="max-w-xl"
	onsubmit={handleCreate}
>
	<div class="flex flex-col gap-5">
		<div role="radiogroup" aria-label={m.wizard_mode_label()} class="grid grid-cols-2 gap-2">
			{#each modes as o (o.mode)}
				{@const picked = mode === o.mode}
				<button
					type="button"
					role="radio"
					aria-checked={picked}
					class={[
						"rounded-field flex cursor-pointer items-start gap-2.5 border p-3 text-left transition-colors",
						picked
							? "border-primary bg-primary/10"
							: "border-base-300 bg-base-100 hover:border-primary/50",
					]}
					onclick={() => (mode = o.mode)}
				>
					<o.Icon
						size={16}
						class={[
							"mt-0.5 shrink-0",
							picked ? "text-primary" : "text-base-content/50",
						]}
					/>
					<span class="min-w-0">
						<span class="block text-sm leading-tight font-medium">{o.title()}</span>
						<span class="text-base-content/60 mt-0.5 block text-xs leading-snug"
							>{o.hint()}</span
						>
					</span>
				</button>
			{/each}
		</div>

		{#if mode === "download"}
			<PatchPicker patches={downloadablePatches} bind:value={downloadId} />
		{:else}
			<FolderImportPanel {importer} />
		{/if}

		{#if needsLogin}
			<LoginMethodPicker bind:value={loginMethod} />
		{/if}

		<div class={["grid gap-3", mode === "download" && "grid-cols-2"]}>
			<label class="flex flex-col gap-1">
				<span class="text-base-content/60 flex justify-between text-xs">
					<span>{m.wizard_instance_name()}</span>
					<span>{m.common_optional()}</span>
				</span>
				<input
					type="text"
					class="input input-sm w-full"
					placeholder={patch ? freeLabel(patch.name) : m.wizard_name_placeholder()}
					bind:value={name}
				/>
			</label>
			{#if mode === "download"}
				<div class="flex flex-col gap-1">
					<label
						for="wizard-install-folder"
						class="text-base-content/60 flex justify-between text-xs"
					>
						<span>{m.wizard_installation_path()}</span>
						<span>{m.common_optional()}</span>
					</label>
					<div class="join w-full">
						<input
							id="wizard-install-folder"
							type="text"
							class="input input-sm join-item min-w-0 flex-1 font-mono"
							spellcheck="false"
							placeholder={defaultInstancePath.value || undefined}
							bind:value={installFolder}
						/>
						<button
							type="button"
							class="btn btn-sm btn-square join-item"
							aria-label={m.common_browse()}
							title={m.common_browse()}
							onclick={browseInstallFolder}
						>
							<FolderOpen size={14} />
						</button>
					</div>
				</div>
				{#if installTarget}
					<p class="text-base-content/60 col-span-2 -mt-1 text-xs wrap-break-word">
						{installTarget.copy
							? m.wizard_installs_copy_to({ path: installTarget.path })
							: m.wizard_installs_to({ path: installTarget.path })}
					</p>
				{/if}
			{/if}
		</div>

		{#if mode === "download" && downloadPatch}
			<details class="group bg-base-100 border-base-300 rounded-field border">
				<summary
					class="text-base-content/70 hover:text-base-content flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm [&::-webkit-details-marker]:hidden"
				>
					<Terminal size={14} />
					{m.wizard_depot_downloader_command()}
					<ChevronDown
						size={14}
						class="ml-auto opacity-60 transition-transform group-open:rotate-180"
					/>
				</summary>
				<div class="border-base-300 flex flex-col gap-2 border-t p-3">
					<p class="text-base-content/60 text-xs">{m.wizard_depot_downloader_hint()}</p>
					<div class="bg-base-200 rounded-field relative">
						<pre
							class="overflow-x-auto py-2 pr-20 pl-3 font-mono text-xs wrap-break-word whitespace-pre-wrap select-text">{depotDownloaderCommand}</pre>
						<button
							type="button"
							class="btn btn-ghost btn-xs absolute top-1.5 right-1.5"
							onclick={handleCopyCommand}
						>
							{#if copyStatus === "copied"}
								<Check size={12} />
								{m.common_copied()}
							{:else}
								<Copy size={12} />
								{copyStatus === "failed" ? m.common_copy_failed() : m.common_copy()}
							{/if}
						</button>
					</div>
				</div>
			</details>
		{/if}
	</div>

	{#snippet actions()}
		<p
			class="text-base-content/55 mr-auto flex min-w-0 items-center gap-1.5 self-center text-xs"
		>
			<Info size={12} class="shrink-0" />
			{mode === "download" ? m.wizard_download_footer() : m.wizard_import_footer()}
		</p>
		<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}>
			{m.common_cancel()}
		</button>
		<button class="btn btn-primary btn-sm" type="submit" disabled={!ready}>
			{#if mode === "download"}
				<CloudDownload size={14} />
				{m.wizard_download_and_create()}
			{:else if importer.detecting}
				<span class="loading loading-spinner loading-xs"></span>
				{m.common_identifying()}
			{:else}
				<FolderOpen size={14} />
				{importer.bulk
					? m.wizard_import_count({ count: importer.chosen.length })
					: m.wizard_import_instance()}
			{/if}
		</button>
	{/snippet}
</Modal>
