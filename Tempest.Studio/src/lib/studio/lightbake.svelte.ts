// Static light baking for the map view: `light.bake` over the open map
// (`lightbake/` in tempest-assets), the bake dialog's options, and the
// selected mesh's light map (`light.info`) for the inspector.

import { map } from "./map.svelte";
import { onUpkEvent, rpc } from "./rpc";
import { toasts } from "./toast.svelte";
import type { LightBakeResult, LightMapInfo } from "../../../electron/src/ipc/contract";

export type BakeScope = "selected" | "edited" | "level";

class LightBakeStore {
	/** The bake dialog. */
	open = $state(false);
	scope = $state<BakeScope>("edited");
	/** `level` scope: level package paths. */
	levels = $state<Record<string, boolean>>({});
	shadowSamples = $state(8);
	skySamples = $state(64);
	indirectRays = $state(0);
	albedo = $state(0.08);
	compare = $state(true);
	busy = $state(false);
	progress = $state<{ done: number; total: number } | null>(null);
	last = $state.raw<LightBakeResult | null>(null);

	/** Bake `scope` (default: the dialog's) and reload the levels written. */
	async bake(scope: BakeScope = this.scope): Promise<LightBakeResult | null> {
		if (!map.path || this.busy) return null;
		const sel = map.selected;
		if (scope === "selected" && !sel) {
			toasts.push("info", "Select a mesh or an actor first", "Bake lighting");
			return null;
		}
		this.busy = true;
		this.progress = null;
		const stop = onUpkEvent((e) => {
			if (e.event === "light.progress") {
				this.progress = { done: Number(e.done), total: Number(e.total) };
			}
		});
		try {
			const r = await rpc("light.bake", {
				path: map.path,
				scope,
				targets:
					scope === "selected" && sel
						? [{ path: sel.level, index: sel.export }]
						: undefined,
				levels:
					scope === "level"
						? Object.entries(this.levels)
								.filter(([, on]) => on)
								.map(([p]) => p)
						: undefined,
				shadowSamples: this.shadowSamples,
				skySamples: this.skySamples,
				indirectRays: this.indirectRays,
				albedo: this.albedo,
				compare: this.compare,
			});
			this.last = r;
			for (const w of r.written) {
				map.noteEdit(w.path);
				await map.reloadLevel(w.path);
			}
			const n = r.baked.length;
			const removed = r.written.reduce((k, w) => k + w.removed.length, 0);
			toasts.push(
				n > 0 ? "success" : "info",
				n > 0
					? `${n} mesh${n === 1 ? "" : "es"} in ${r.written.length} level${r.written.length === 1 ? "" : "s"}, ${r.seconds.toFixed(1)} s${r.skipped.length ? `; ${r.skipped.length} skipped` : ""}${removed ? `; ${removed} old light map texture${removed === 1 ? "" : "s"} removed` : ""}`
					: r.skipped.length
						? `Nothing baked: ${r.skipped[0].reason}`
						: "Nothing to bake in this scope",
				"Lighting baked",
			);
			return r;
		} catch (error) {
			toasts.error(error, "Could not bake the lighting");
			return null;
		} finally {
			stop();
			this.busy = false;
		}
	}

	/** The light map of a static mesh component, with an irradiance preview. */
	async info(level: string, index: number, exposure = 1): Promise<LightMapInfo> {
		return await rpc("light.info", { path: level, index, preview: true, exposure });
	}
}

export const lightBake = new LightBakeStore();
