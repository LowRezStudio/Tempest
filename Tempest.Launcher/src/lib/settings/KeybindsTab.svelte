<script lang="ts">
	import {
		File,
		FlaskConical,
		FolderOpen,
		MousePointer2,
		Plus,
		RotateCcw,
		Trash2,
	} from "@lucide/svelte";
	import { save as saveDialog } from "@tauri-apps/plugin-dialog";
	import { path } from "@tauri-apps/api";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { goto } from "$app/navigation";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import { installMod } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { keybinds, resetKeybinds, cookKeybindsMod } from "$lib/stores/keybinds.svelte";
	import { addToast, removeToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	const queryClient = useQueryClient();

	let modName = $state("Keybinds");
	let modVersion = "1.0.0";

	let cooking = $state(false);
	let cookedZip = $state<Uint8Array | null>(null);
	let showInstanceSelect = $state(false);
	let installingLabel = $state<string | null>(null);
	let capturingIndex = $state<number | null>(null);

	let hasBindings = $derived(keybinds.value.bindings.some((b) => b.name.trim() && b.command.trim()));

	function updateSensitivity(e: Event) {
		const value = Number((e.target as HTMLInputElement).value);
		const rounded = Number.isFinite(value) ? Math.round(value * 10) / 10 : 0;
		keybinds.value = { ...keybinds.value, mouseSensitivity: rounded };
	}

	function addBinding() {
		keybinds.value = {
			...keybinds.value,
			bindings: [...keybinds.value.bindings, { name: "", command: "" }],
		};
	}

	function updateBinding(index: number, patch: Partial<{ name: string; command: string }>) {
		const bindings = keybinds.value.bindings.map((binding, i) =>
			i === index ? { ...binding, ...patch } : binding,
		);
		keybinds.value = { ...keybinds.value, bindings };
	}

	function keyLabel(e: KeyboardEvent): string {
		if (e.key.length === 1) return e.key.toUpperCase();

		const modifiers: Record<string, string> = {
			ShiftLeft: "LeftShift",
			ShiftRight: "RightShift",
			ControlLeft: "LeftControl",
			ControlRight: "RightControl",
			AltLeft: "LeftAlt",
			AltRight: "RightAlt",
		};
		if (modifiers[e.code]) return modifiers[e.code];

		const names: Record<string, string> = {
			" ": "SpaceBar",
			ArrowUp: "Up",
			ArrowDown: "Down",
			ArrowLeft: "Left",
			ArrowRight: "Right",
			Enter: "Enter",
			Escape: "Escape",
			Tab: "Tab",
			Backspace: "Backspace",
			Delete: "Delete",
			Shift: "Shift",
			Control: "Ctrl",
			Alt: "Alt",
			CapsLock: "CapsLock",
			Home: "Home",
			End: "End",
			PageUp: "PageUp",
			PageDown: "PageDown",
			Insert: "Insert",
		};
		return names[e.key] ?? e.key;
	}

	function mouseKeyLabel(button: number): string | null {
		switch (button) {
			case 0: {
				return "LeftMouseButton";
			}
			case 1: {
				return "MiddleMouseButton";
			}
			case 2: {
				return "RightMouseButton";
			}
			case 3: {
				return "ThumbMouseButton";
			}
			case 4: {
				return "ThumbMouseButton2";
			}
			default: {
				return null;
			}
		}
	}

	function beginCapture(index: number) {
		capturingIndex = capturingIndex === index ? null : index;
	}

	function endCapture() {
		capturingIndex = null;
	}

	function handleCaptureKeydown(e: KeyboardEvent) {
		if (capturingIndex === null) return;
		e.preventDefault();
		e.stopPropagation();

		if (e.key === "Escape") {
			endCapture();
			return;
		}
		if (e.key === "Backspace" || e.key === "Delete") {
			updateBinding(capturingIndex, { name: "" });
			endCapture();
			return;
		}
		if (e.key === "Tab") {
			endCapture();
			return;
		}

		updateBinding(capturingIndex, { name: keyLabel(e) });
		endCapture();
	}

	function handleCaptureMousedown(e: MouseEvent) {
		if (capturingIndex === null) return;
		e.preventDefault();
		e.stopPropagation();

		const label = mouseKeyLabel(e.button);
		if (label) {
			updateBinding(capturingIndex, { name: label });
		}
		endCapture();
	}

	$effect(() => {
		if (capturingIndex === null) return;
		const onKey = (e: KeyboardEvent) => handleCaptureKeydown(e);
		const onMouse = (e: MouseEvent) => handleCaptureMousedown(e);
		window.addEventListener("keydown", onKey);
		window.addEventListener("mousedown", onMouse);
		return () => {
			window.removeEventListener("keydown", onKey);
			window.removeEventListener("mousedown", onMouse);
		};
	});

	function removeBinding(index: number) {
		keybinds.value = {
			...keybinds.value,
			bindings: keybinds.value.bindings.filter((_, i) => i !== index),
		};
	}

	async function cook() {
		if (!hasBindings) return;
		cooking = true;
		cookedZip = null;
		try {
			cookedZip = await cookKeybindsMod(keybinds.value, {
				name: modName,
				version: modVersion,
				authors: [{ name: "Tempest", link: "" }],
			});
		} catch (error) {
			console.error("Failed to cook keybinds mod:", error);
		} finally {
			cooking = false;
		}
	}

	async function download() {
		if (!cookedZip) return;
		const savePath = await saveDialog({
			defaultPath: `${modName.trim() || "keybinds"}.tempest`,
			filters: [{ name: "Tempest Mod", extensions: ["tempest"] }],
		});
		if (!savePath) return;
		try {
			const { writeFile } = await import("@tauri-apps/plugin-fs");
			await writeFile(savePath, cookedZip);
			addToast({
				title: m.converter_toast_saved_title(),
				message: m.converter_toast_saved_message({ path: savePath }),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to save keybinds mod:", error);
		}
	}

	async function installToInstance(inst: Instance) {
		if (!cookedZip || !inst?.path) return;
		const modFileName = modName.trim() || "keybinds";
		installingLabel = modFileName;
		void goto(`/instance/${inst.id}`);

		let installingToastId: string | undefined;
		try {
			installingToastId = addToast({
				title: m.toast_mod_installing_title(),
				message: m.toast_mod_installing_message({ name: modFileName }),
				tone: "info",
				duration: 0,
			});

			const { writeFile } = await import("@tauri-apps/plugin-fs");
			const tmpDir = await path.tempDir();
			const tmpFile = await path.join(tmpDir, `${modFileName}.tempest`);
			await writeFile(tmpFile, cookedZip);
			const result = await installMod(inst.path, tmpFile, true, true);

			if (installingToastId) removeToast(installingToastId);

			if (result.Success) {
				addToast({
					title: m.toast_mod_installed_title(),
					message: m.converter_toast_installed_message({ name: modName, instance: inst.label }),
					tone: "success",
				});
				queryClient.invalidateQueries({ queryKey: ["mods", inst.path] });
			} else {
				addToast({
					title: m.toast_installation_failed_title(),
					message: result.Message || m.common_unknown(),
					tone: "error",
				});
			}
			try {
				await import("@tauri-apps/plugin-fs").then((m) => m.remove(tmpFile));
			} catch {}
		} catch (error) {
			if (installingToastId) removeToast(installingToastId);
			console.error("Failed to install keybinds mod:", error);
		} finally {
			installingLabel = null;
		}
	}

	function addToInstance() {
		if (!cookedZip) return;
		const instanceId = window.location.pathname.match(/^\/instance\/([^/]+)/)?.[1];
		if (instanceId && instanceMap.value[instanceId]?.path) {
			void installToInstance(instanceMap.value[instanceId]);
		} else {
			showInstanceSelect = true;
		}
	}
</script>

<div class="flex flex-col gap-6">
	<div class="alert">
		<MousePointer2 size={20} />
		<div>
			<p class="text-sm opacity-90">{m.settings_keybinds_description()}</p>
			<p class="text-xs opacity-60 mt-1">{m.settings_keybinds_unsaved()}</p>
		</div>
	</div>

	<section class="flex flex-col gap-2 max-w-sm">
		<h4 class="text-xs font-semibold uppercase opacity-60">
			{m.settings_keybinds_mouse_sensitivity()}
		</h4>
		<div class="flex items-center gap-3">
			<input
				type="range"
				class="range range-sm flex-1"
				value={keybinds.value.mouseSensitivity}
				oninput={updateSensitivity}
				min="0"
				max="50"
				step="0.1"
			/>
			<input
				type="number"
				class="input input-bordered input-sm w-20 font-mono text-center"
				value={keybinds.value.mouseSensitivity}
				oninput={updateSensitivity}
				min="0"
				max="50"
				step="0.1"
			/>
		</div>
	</section>

	<section class="flex flex-col gap-2">
		<div class="flex items-center justify-between">
			<h4 class="text-xs font-semibold uppercase opacity-60">
				{m.settings_keybinds_action()}
			</h4>
			<button type="button" class="btn btn-ghost btn-xs" onclick={addBinding}>
				<Plus size={14} />
				{m.settings_keybinds_add()}
			</button>
		</div>

		{#each keybinds.value.bindings as binding, i (i)}
			<div class="flex items-center gap-2 p-2 rounded-box bg-base-200">
				<button
					type="button"
					class="btn btn-sm w-32 font-mono uppercase shrink-0"
					class:btn-accent={capturingIndex === i}
					onclick={() => beginCapture(i)}
					title={m.settings_keybinds_key()}
				>
					{#if capturingIndex === i}
						<span class="normal-case">{m.settings_keybinds_press_a_key()}</span>
					{:else}
						{binding.name || "?"}
					{/if}
				</button>
				<input
					type="text"
					class="input input-bordered input-sm flex-1 min-w-0 font-mono"
					value={binding.command}
					placeholder="GBA_..."
					oninput={(e) =>
						updateBinding(i, { command: (e.target as HTMLInputElement).value })}
				/>
				<button
					type="button"
					class="btn btn-ghost btn-xs btn-square shrink-0"
					onclick={() => removeBinding(i)}
					aria-label={m.settings_keybinds_remove()}
					title={m.settings_keybinds_remove()}
				>
					<Trash2 size={14} />
				</button>
			</div>
		{/each}
	</section>

	<div class="divider my-0"></div>

	<section class="flex flex-col gap-4">
		<div class="flex flex-col gap-1 max-w-sm">
			<span class="text-sm font-semibold">{m.settings_keybinds_mod_name()}</span>
			<input
				type="text"
				class="input input-bordered w-full"
				bind:value={modName}
				placeholder={m.settings_keybinds_mod_name_placeholder()}
			/>
		</div>

		<div class="flex flex-wrap items-center gap-2">
			<button type="button" class="btn btn-accent gap-2" disabled={!hasBindings || cooking} onclick={cook}>
				{#if cooking}
					<span class="loading loading-spinner loading-xs"></span>
					{m.settings_keybinds_generating()}
				{:else}
					<FlaskConical size={18} />
					{m.settings_keybinds_generate()}
				{/if}
			</button>

			{#if cookedZip}
				<button type="button" class="btn btn-accent gap-2" onclick={download}>
					<FolderOpen size={16} />
					{m.converter_download()}
				</button>
				<button type="button" class="btn btn-accent gap-2" onclick={addToInstance} disabled={!!installingLabel}>
					<File size={16} />
					{m.converter_add_to_instance()}
				</button>
			{/if}
		</div>
	</section>

	<div>
		<button type="button" class="btn btn-error btn-sm" onclick={resetKeybinds}>
			<RotateCcw size={14} />
			{m.settings_keybinds_reset()}
		</button>
	</div>
</div>

<InstanceSelectModal
	bind:open={showInstanceSelect}
	onselect={(inst) => {
		void installToInstance(inst);
	}}
	oncancel={() => {
		showInstanceSelect = false;
	}}
/>
