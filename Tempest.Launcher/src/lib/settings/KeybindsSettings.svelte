<script lang="ts">
	import { goto } from "$app/navigation";
	import { File, FlaskConical, FolderOpen, Plus, X } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import {
		cookKeybindsMod,
		discardStagedModPack,
		installMod,
		saveStagedModPack,
	} from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { keybinds } from "$lib/stores/keybinds.svelte";
	import { addToast, removeToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	const queryClient = useQueryClient();

	let modName = $state("Keybinds");
	let modVersion = "1.0.0";

	let cooking = $state(false);
	/** Host-staged cooked package; the newest cook replaces the previous one. */
	let stagedPath = $state<string | null>(null);
	let showInstanceSelect = $state(false);
	let installingLabel = $state<string | null>(null);
	let capturingIndex = $state<number | null>(null);

	let hasBindings = $derived(
		keybinds.value.bindings.some((b) => b.name.trim() && b.command.trim()),
	);

	// The legacy theme paints sliders with the progress-bar artwork, which needs
	// the fill position as a CSS property.
	const sliderFill = (value: number, min: number, max: number) =>
		`${((value - min) / (max - min)) * 100}%`;

	function updateSensitivity(e: Event) {
		const value = Number((e.target as HTMLInputElement).value);
		const rounded = Number.isFinite(value) ? Math.round(value * 10) / 10 : 0;
		keybinds.value = { ...keybinds.value, mouseSensitivity: rounded };
	}

	function updateFpsLimit(e: Event) {
		keybinds.value = {
			...keybinds.value,
			fpsLimit: Number((e.target as HTMLInputElement).value),
		};
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
		const previous = stagedPath;
		stagedPath = null;
		try {
			stagedPath = await cookKeybindsMod(modName, modVersion, keybinds.value);
			// Only the newest cook stays staged; an install may still be reading the
			// previous package, so it is dropped here rather than on unmount.
			if (previous && previous !== stagedPath) {
				void discardStagedModPack(previous).catch(() => {});
			}
		} catch (error) {
			console.error("Failed to cook keybinds mod:", error);
		} finally {
			cooking = false;
		}
	}

	async function download() {
		if (!stagedPath) return;
		try {
			// Save dialog + copy happen host-side in one step.
			const savedPath = await saveStagedModPack(stagedPath);
			if (!savedPath) return;
			addToast({
				title: m.converter_toast_saved_title(),
				message: m.converter_toast_saved_message({ path: savedPath }),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to save keybinds mod:", error);
		}
	}

	async function installToInstance(inst: Instance) {
		if (!stagedPath || !inst?.path) return;
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

			// The cooked package is already staged in the temp dir; the CLI installs
			// from that path and the renderer never touches the filesystem.
			const result = await installMod(inst.path, stagedPath, true);

			if (installingToastId) removeToast(installingToastId);

			if (result.Success) {
				addToast({
					title: m.toast_mod_installed_title(),
					message: m.converter_toast_installed_message({
						name: modName,
						instance: inst.label,
					}),
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
		} catch (error) {
			if (installingToastId) removeToast(installingToastId);
			console.error("Failed to install keybinds mod:", error);
		} finally {
			installingLabel = null;
		}
	}

	function addToInstance() {
		if (!stagedPath) return;
		const instanceId = window.location.pathname.match(/^\/instance\/([^/]+)/)?.[1];
		if (instanceId && instanceMap.value[instanceId]?.path) {
			void installToInstance(instanceMap.value[instanceId]);
		} else {
			showInstanceSelect = true;
		}
	}
</script>

<div class="flex flex-col gap-4">
	<p class="text-base-content/60 text-sm">
		{m.settings_keybinds_description()}
		{m.settings_keybinds_unsaved()}
	</p>

	<div class="grid gap-3 @lg:grid-cols-2">
		<div class="flex flex-col gap-1">
			<span class="text-base-content/60 flex min-h-5 items-center text-xs"
				>{m.settings_keybinds_mouse_sensitivity()}</span
			>
			<div class="flex items-center gap-2">
				<input
					type="range"
					class="range range-xs range-primary flex-1"
					aria-label={m.settings_keybinds_mouse_sensitivity()}
					value={keybinds.value.mouseSensitivity}
					oninput={updateSensitivity}
					min="0"
					max="50"
					step="0.1"
				/>
				<input
					type="number"
					class="input input-sm w-20 text-center font-mono"
					aria-label={m.settings_keybinds_mouse_sensitivity()}
					value={keybinds.value.mouseSensitivity}
					oninput={updateSensitivity}
					min="0"
					max="50"
					step="0.1"
				/>
			</div>
		</div>
		<div class="flex flex-col gap-1">
			<span class="text-base-content/60 flex min-h-5 items-center justify-between text-xs">
				{m.settings_keybinds_fps_limit()}
				<label class="flex cursor-pointer items-center gap-1.5">
					<input
						type="checkbox"
						class="checkbox checkbox-xs"
						bind:checked={keybinds.value.unlockFps}
					/>
					{m.settings_keybinds_unlock_fps()}
				</label>
			</span>
			<div class="flex items-center gap-2">
				<input
					type="range"
					class="range range-xs range-primary flex-1"
					aria-label={m.settings_keybinds_fps_limit()}
					value={keybinds.value.fpsLimit}
					oninput={updateFpsLimit}
					min="30"
					max="1000"
					step="1"
					disabled={keybinds.value.unlockFps}
				/>
				<input
					type="number"
					class="input input-sm w-20 text-center font-mono"
					aria-label={m.settings_keybinds_fps_limit()}
					value={keybinds.value.fpsLimit}
					oninput={updateFpsLimit}
					min="30"
					max="1000"
					step="1"
					disabled={keybinds.value.unlockFps}
				/>
			</div>
		</div>
	</div>

	<div class="flex flex-col gap-2">
		<div class="flex items-center gap-2 text-xs">
			<span class="text-base-content/60">{m.settings_keybinds_action()}</span>
			<span class="text-base-content/40 tabular-nums">{keybinds.value.bindings.length}</span>
			<button type="button" class="btn btn-ghost btn-xs ml-auto" onclick={addBinding}>
				<Plus size={12} />
				{m.settings_keybinds_add()}
			</button>
		</div>
		<ul class="grid gap-2 @2xl:grid-cols-2">
			{#each keybinds.value.bindings as binding, i (i)}
				<li
					class="bg-base-100 border-base-300 rounded-field flex items-center gap-2 border p-1.5"
				>
					<button
						type="button"
						class={[
							"btn btn-sm w-32 shrink-0 font-mono text-xs",
							capturingIndex === i ? "btn-primary" : "uppercase",
						]}
						onclick={() => beginCapture(i)}
						title={m.settings_keybinds_key()}
					>
						{capturingIndex === i
							? m.settings_keybinds_press_a_key()
							: binding.name || "?"}
					</button>
					<input
						type="text"
						class="input input-sm min-w-0 flex-1 font-mono"
						value={binding.command}
						placeholder="GBA_..."
						spellcheck="false"
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
						<X size={14} />
					</button>
				</li>
			{/each}
		</ul>
	</div>

	<div class="border-base-300 flex flex-col gap-1 border-t pt-4">
		<label for="keybinds-mod-name" class="text-base-content/60 text-xs"
			>{m.settings_keybinds_mod_name()}</label
		>
		<div class="flex flex-wrap items-center gap-2">
			<input
				id="keybinds-mod-name"
				type="text"
				class="input input-sm w-56"
				bind:value={modName}
				placeholder={m.settings_keybinds_mod_name_placeholder()}
			/>
			<button
				type="button"
				class="btn btn-primary btn-sm"
				disabled={!hasBindings || cooking}
				onclick={cook}
			>
				{#if cooking}
					<span class="loading loading-spinner loading-xs"></span>
					{m.settings_keybinds_generating()}
				{:else}
					<FlaskConical size={14} />
					{m.settings_keybinds_generate()}
				{/if}
			</button>
			{#if stagedPath}
				<button type="button" class="btn btn-sm" onclick={download}>
					<FolderOpen size={14} />
					{m.converter_download()}
				</button>
				<button
					type="button"
					class="btn btn-sm"
					onclick={addToInstance}
					disabled={!!installingLabel}
				>
					<File size={14} />
					{m.converter_add_to_instance()}
				</button>
			{/if}
		</div>
		<p class="text-base-content/60 text-xs">{m.settings_keybinds_mod_hint()}</p>
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
