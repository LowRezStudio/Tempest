import type { Instance } from "$lib/types/instance";

export function getInstanceColor(instance: Instance | undefined): string {
	if (!instance) return "#808080";
	if (instance.color) return instance.color;
	let hash = 0;
	const name = instance.label || "";
	for (let i = 0; i < name.length; i++) {
		hash = name.charCodeAt(i) + ((hash << 5) - hash);
	}
	const h = Math.abs(hash) % 360;
	const s = 0.65;
	const l = 0.55;
	const a = s * Math.min(l, 1 - l);
	const f = (n: number) => {
		const k = (n + h / 30) % 12;
		const color = l - a * Math.max(Math.min(k - 3, 9 - k, 1), -1);
		return Math.round(255 * color)
			.toString(16)
			.padStart(2, "0");
	};
	return `#${f(0)}${f(8)}${f(4)}`;
}

export function getMutedInstanceColor(instance: Instance | undefined): string {
	return `color-mix(in oklab, ${getInstanceColor(instance)} 60%, var(--color-base-content))`;
}

export function getContrastColor(hex: string | undefined): string {
	if (!hex) return "#ffffff";
	const color = hex.replace("#", "");
	if (color.length !== 6) return "#ffffff";
	const r = Number.parseInt(color.substring(0, 2), 16);
	const g = Number.parseInt(color.substring(2, 4), 16);
	const b = Number.parseInt(color.substring(4, 6), 16);
	const yiq = (r * 299 + g * 587 + b * 114) / 1000;
	return yiq >= 150 ? "#121212" : "#ffffff";
}
