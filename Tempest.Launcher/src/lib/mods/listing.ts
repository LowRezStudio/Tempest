// How repository mods are presented, matching the mod website: plain-language trust labels,
// the card image, the icon fallback and the short facts on a row.
import { BadgeCheck, FileBox, ShieldAlert, ShieldCheck, ShieldX } from "@lucide/svelte";
import { m } from "$lib/paraglide/messages";
import { getLocale } from "$lib/paraglide/runtime";
import type { RepoImage, RepoMod, RepoSigner, RepoVersion } from "$lib/queries/repo";

export type Trust = {
	label: string;
	hint: string;
	tone: "primary" | "success" | "warning" | "error" | "neutral";
	icon: typeof BadgeCheck;
};

// One trust label instead of signer, key and review details.
export function trustOf(latest: RepoVersion | null): Trust {
	const signer = latest?.signer;
	if (!signer) {
		return {
			label: m.repo_trust_unsigned(),
			hint: m.repo_trust_unsigned_hint(),
			tone: "neutral",
			icon: FileBox,
		};
	}
	const byTrust: Record<RepoSigner["trust"], () => Trust> = {
		"first-party": () => ({
			label: m.repo_trust_official(),
			hint: m.repo_trust_official_hint(),
			tone: "primary",
			icon: BadgeCheck,
		}),
		trusted: () => ({
			label: m.repo_trust_verified(),
			hint: m.repo_trust_verified_hint({ name: signer.name }),
			tone: "success",
			icon: ShieldCheck,
		}),
		revoked: () => ({
			label: m.repo_trust_revoked(),
			hint: m.repo_trust_revoked_hint(),
			tone: "error",
			icon: ShieldX,
		}),
		unknown: () => ({
			label: m.repo_trust_reviewed(),
			hint: m.repo_trust_reviewed_hint({ name: signer.name }),
			tone: "warning",
			icon: ShieldAlert,
		}),
	};
	return byTrust[signer.trust]();
}

export const toneBadge: Record<Trust["tone"], string> = {
	primary: "badge-primary",
	success: "badge-success",
	warning: "badge-warning",
	error: "badge-error",
	neutral: "badge-ghost",
};

export const kindLabel = (kind: RepoMod["kind"]) =>
	({
		assets: m.repo_kind_assets(),
		scripts: m.repo_kind_scripts(),
		native: m.repo_kind_native(),
	})[kind];

export const kindHint = (kind: RepoMod["kind"]) =>
	({
		assets: m.repo_kind_assets_hint(),
		scripts: m.repo_kind_scripts_hint(),
		native: m.repo_kind_native_hint(),
	})[kind];

// The picture on a browse row: the first screenshot, else the banner.
export function cardImageOf(mod: RepoMod): RepoImage | null {
	const assets = mod.latest?.assets;
	return assets?.screenshots[0] ?? assets?.banner ?? null;
}

// What stands in for a missing mod icon: the name's initials on a gradient hued by the mod id.
export function modIconFallback(id: string, name: string) {
	let hash = 0;
	for (const ch of id) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
	const hue = hash % 360;
	const initials = name
		.split(/\s+/)
		.slice(0, 2)
		.map((w) => w[0]?.toUpperCase() ?? "")
		.join("");
	return {
		initials,
		background: `linear-gradient(135deg,hsl(${hue} 55% 45%),hsl(${(hue + 40) % 360} 60% 30%))`,
	};
}

export function formatCount(n: number): string {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 10_000) return `${Math.round(n / 1000)}k`;
	if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
	return String(n);
}

export function relativeDate(iso: string, now = new Date()): string {
	let diff = (new Date(iso).getTime() - now.getTime()) / 1000;
	const rtf = new Intl.RelativeTimeFormat(getLocale(), { numeric: "auto" });
	const steps: [number, Intl.RelativeTimeFormatUnit][] = [
		[60, "second"],
		[60, "minute"],
		[24, "hour"],
		[7, "day"],
		[4.35, "week"],
		[12, "month"],
	];
	for (const [size, unit] of steps) {
		if (Math.abs(diff) < size) return rtf.format(Math.round(diff), unit);
		diff /= size;
	}
	return rtf.format(Math.round(diff), "year");
}
