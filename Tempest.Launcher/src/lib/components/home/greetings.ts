// The home page's greeting and the quip under it: by time of day, weekday, season, the patch
// you're on and what the launcher knows about you. Mostly jokes.
import champions from "$lib/data/champions.json";
import maps from "$lib/data/maps.json";
import { m } from "$lib/paraglide/messages";
import { isAutumn } from "$lib/theme/weather.svelte";
import { compareVersions, getMapsForVersion } from "$lib/utils/versions";
import type { Weather } from "$lib/theme/weather.svelte";

export type GreetingContext = {
	now: Date;
	name: string;
	/** The hero's instance */
	instance?: {
		label: string;
		version?: string;
		lastPlayedAt?: number;
		patchName?: string;
		patchDate?: string;
	};
	running: boolean;
	instanceCount: number;
	downloading: boolean;
};

type Message = (inputs?: Record<string, unknown>) => string;
const messages = m as unknown as Record<string, Message | undefined>;
const keysByPrefix = new Map<string, string[]>();

// Quips are found by key, so a new `<prefix>_<n>` in messages/en.json joins its pool by itself.
function pool(prefix: string, inputs?: Record<string, unknown>): string[] {
	let keys = keysByPrefix.get(prefix);
	if (!keys) {
		const pattern = new RegExp(`^${prefix}(_\\d+)?$`);
		keys = Object.keys(messages).filter((key) => pattern.test(key));
		keysByPrefix.set(prefix, keys);
	}
	return keys.map((key) => messages[key]!(inputs));
}

const pick = <T>(from: T[]) => from[Math.floor(Math.random() * from.length)];

type Season =
	| "winter"
	| "christmas"
	| "newYearsEve"
	| "newYear"
	| "halloween"
	| "aprilFools"
	| "valentine"
	| "leapDay"
	| "pirate"
	| "paladinsBirthday";

// Paladins 1.0
const RELEASE = { year: 2018, month: 5, day: 8 };

function seasonOf(date: Date): Season | undefined {
	const month = date.getMonth() + 1;
	const day = date.getDate();
	if (month === 12 && day >= 24 && day <= 26) return "christmas";
	if (month === 12 && day === 31) return "newYearsEve";
	if (month === 12) return "winter";
	if (month === 1 && day === 1) return "newYear";
	if (month === 10 && day >= 24) return "halloween";
	if (month === 4 && day === 1) return "aprilFools";
	if (month === 2 && day === 14) return "valentine";
	if (month === 2 && day === 29) return "leapDay";
	// Talk Like a Pirate Day, and the day Plunder Ahoy! came out
	if (month === 9 && day === 19) return "pirate";
	if (month === RELEASE.month && day === RELEASE.day) return "paladinsBirthday";
	return undefined;
}

/**
 * The page's heading: the time of day, or the holiday.
 * @param ctx - What the greeting may use.
 * @returns E.g. `Good evening, Shiro`.
 */
export function greetingTitle(ctx: GreetingContext): string {
	const { now, name } = ctx;
	const year = String(now.getFullYear());
	const season = seasonOf(now);
	if (season === "christmas") return m.home_greeting_christmas({ name });
	if (season === "newYearsEve") return m.home_greeting_new_years_eve({ name, year });
	if (season === "newYear") return m.home_greeting_new_year({ name, year });
	if (season === "halloween" && now.getDate() === 31) return m.home_greeting_halloween({ name });
	const hour = now.getHours();
	if (hour < 5) return m.home_greeting_night({ name });
	if (hour < 12) return m.home_greeting_morning({ name });
	if (hour < 18) return m.home_greeting_afternoon({ name });
	return m.home_greeting_evening({ name });
}

const occasions: Record<Season, string> = {
	winter: "home_quip_winter",
	christmas: "home_quip_christmas",
	newYearsEve: "home_quip_nye",
	newYear: "home_quip_new_year",
	halloween: "home_quip_halloween",
	aprilFools: "home_quip_april",
	valentine: "home_quip_valentine",
	leapDay: "home_quip_leap_day",
	pirate: "home_quip_pirate",
	paladinsBirthday: "home_quip_paladins_birthday",
};

// Stretches of the game's life a patch can belong to; they overlap.
const stretches = [
	{ prefix: "home_quip_no_mode_onslaught", from: "0.33", to: "0.51" },
	{ prefix: "home_quip_no_mode_tdm", from: "0.33", to: "0.64" },
	{ prefix: "home_quip_no_mode_payload", from: "0.65", to: "5.7" },
	{ prefix: "home_quip_era_open_beta", from: "0.33", to: "0.43" },
	{ prefix: "home_quip_era_legendary_essence", from: "0.44", to: "0.63" },
	{ prefix: "home_quip_era_cards_unbound", from: "0.64", to: "0.66" },
	{ prefix: "home_quip_era_battlegrounds", from: "0.66", to: "0.70" },
	{ prefix: "home_quip_era_launch", from: "0.67", to: "1.9" },
	{ prefix: "home_quip_era_crossplay_season2", from: "2.1", to: "2.8" },
	{ prefix: "home_quip_era_bugfest", from: "3.1", to: "4.7" },
	{ prefix: "home_quip_era_late_life", from: "5.1", to: "6.6" },
	{ prefix: "home_quip_era_final_years", from: "7.1", to: "8.2" },
];

const DAY = 86_400_000;

const championsOf = (version: string | undefined) =>
	version ? champions.filter((c) => compareVersions(version, c.version) >= 0) : champions;

// 8.2 never shipped and has no map list of its own; it gets every map.
function mapsOf(version: string | undefined) {
	const own = version ? getMapsForVersion(version) : [];
	return own.length > 0 ? own : maps;
}

// Reworks (R), the originals brought back (Og) and Stone Keep by day are the same place.
const placeOf = (map: { displayName: string }) =>
	map.displayName.replaceAll(/^Og |\(R\)| Day$/g, "").trim();
const slugOf = (name: string) =>
	name
		.toLowerCase()
		.replaceAll("'", "")
		.replaceAll(/[^a-z0-9]+/g, "_");

// Betty La Bomba goes by Betty.
const shortName = (champion: { displayName: string }) => champion.displayName.split(" La ")[0]!;

// A quip naming a champion or map the patch doesn't have yet waits for a patch that does.
function fitsPatch(version: string | undefined): (quip: string) => boolean {
	if (!version) return () => true;
	const present = new Set(mapsOf(version).map(placeOf));
	const absent = [
		...champions.filter((c) => compareVersions(version, c.version) < 0).map(shortName),
		...new Set(maps.map(placeOf).filter((place) => !present.has(place))),
	];
	if (absent.length === 0) return () => true;
	const pattern = new RegExp(`\\b(${absent.join("|")})\\b`);
	return (quip) => !pattern.test(quip);
}

// The holiday, the clock striking something and the patch's birthday.
function occasionQuips(ctx: GreetingContext): string[] {
	const { now, instance } = ctx;
	const season = seasonOf(now);
	const quips = season
		? pool(occasions[season], { years: now.getFullYear() - RELEASE.year })
		: [];
	if (now.getDay() === 5 && now.getDate() === 13) quips.push(...pool("home_quip_friday13"));
	const hour = now.getHours();
	const minute = now.getMinutes();
	if (hour === 12 && minute < 5) quips.push(...pool("home_quip_high_noon"));
	if (hour === 13 && minute === 37) quips.push(...pool("home_quip_leet"));
	const released = instance?.patchDate ? new Date(instance.patchDate) : undefined;
	if (
		released &&
		released.getMonth() === now.getMonth() &&
		released.getDate() === now.getDate() &&
		released.getFullYear() < now.getFullYear()
	) {
		const years = now.getFullYear() - released.getFullYear();
		quips.push(m.home_quip_birthday({ patch: instance?.patchName ?? "", years }));
	}
	return quips;
}

// What the launcher knows about you: downloads, a long break, a museum of patches, your name.
function situationQuips(ctx: GreetingContext): string[] {
	const { now, name, instance } = ctx;
	const quips: string[] = [];
	if (ctx.instanceCount === 0) quips.push(...pool("home_quip_no_instances"));
	if (ctx.instanceCount >= 5) {
		quips.push(...pool("home_quip_collector", { count: ctx.instanceCount }));
	}
	if (ctx.downloading) quips.push(...pool("home_quip_downloading"));
	if (instance && !instance.lastPlayedAt) {
		quips.push(...pool("home_quip_never_played", { name: instance.label }));
	} else if (instance?.lastPlayedAt && now.getTime() - instance.lastPlayedAt > 30 * DAY) {
		const days = Math.floor((now.getTime() - instance.lastPlayedAt) / DAY);
		quips.push(...pool("home_quip_long_absence"), ...pool("home_quip_days_away", { days }));
	}
	const namesake = champions.find(
		(c) => c.displayName.toLowerCase() === name.trim().toLowerCase(),
	);
	if (namesake) {
		quips.push(
			...pool("home_quip_namesake", { name }),
			...pool(`home_quip_champ_${namesake.name}`),
		);
	}
	return quips;
}

// The patch you're on: its own jokes, its era, its new champion, the modes and champions to come.
function patchQuips(ctx: GreetingContext, fits: (quip: string) => boolean): string[] {
	const version = ctx.instance?.version;
	if (!version) return [];
	const quips = [
		...pool(`home_quip_patch_${version.replace(".", "_")}`),
		...stretches
			.filter(
				(s) => compareVersions(version, s.from) >= 0 && compareVersions(version, s.to) <= 0,
			)
			.flatMap((s) => pool(s.prefix)),
		...champions
			.filter((c) => c.version === version)
			.flatMap((c) => pool("home_quip_new_champion", { champion: c.displayName })),
	].filter(fits);
	const later = champions.filter((c) => compareVersions(version, c.version) < 0);
	if (later.length > 0) {
		quips.push(...pool("home_quip_missing_champion", { champion: pick(later)!.displayName }));
	}
	return quips;
}

// The time, the weekday, the season, the patch's age and your name.
function momentQuips(ctx: GreetingContext): string[] {
	const { now, name, instance } = ctx;
	const hour = now.getHours();
	const weekday = now.getDay();
	const month = now.getMonth() + 1;
	const prefixes: string[] = [];
	if (hour < 5) prefixes.push("home_quip_late");
	else if (hour < 10) prefixes.push("home_quip_morning");
	else if (hour >= 12 && hour < 14) prefixes.push("home_quip_lunch");
	else if (hour >= 14 && hour < 18) prefixes.push("home_quip_afternoon");
	else if (hour >= 18) prefixes.push("home_quip_evening");
	if (weekday === 1 && hour < 12) prefixes.push("home_quip_monday");
	if (weekday === 5 && hour >= 17) prefixes.push("home_quip_friday");
	if (weekday === 0 || weekday === 6) prefixes.push("home_quip_weekend");
	if (weekday === 0 && hour >= 18) prefixes.push("home_quip_sunday");
	// December belongs to the winter occasion
	if (isAutumn(now)) prefixes.push("home_quip_autumn");
	else if (month <= 2) prefixes.push("home_quip_midwinter");
	else if (month <= 5) prefixes.push("home_quip_spring");
	else if (month <= 9) prefixes.push("home_quip_summer");
	if (name === "Player") prefixes.push("home_quip_default_name");
	const year = instance?.patchDate ? new Date(instance.patchDate).getFullYear() : undefined;
	if (year && now.getFullYear() - year >= 5) prefixes.push("home_quip_vintage");
	const time = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
	return prefixes.flatMap((prefix) => pool(prefix, { time, year }));
}

// What always fits: a pile of Paladins jokes, and those about the champions, maps and modes
// of your patch.
function everydayQuips(ctx: GreetingContext): string[] {
	const version = ctx.instance?.version;
	const patchMaps = mapsOf(version);
	return [
		...pool("home_quip"),
		...pool("home_quip_tempest"),
		...championsOf(version).flatMap((c) => pool(`home_quip_champ_${c.name}`)),
		...[...new Set(patchMaps.map((map) => slugOf(placeOf(map))))].flatMap((slug) =>
			pool(`home_quip_map_${slug}`),
		),
		...[...new Set(patchMaps.map((map) => map.mode))].flatMap((mode) =>
			pool(`home_quip_mode_${mode}`),
		),
	];
}

const recent: string[] = [];

// Leaves out the quips shown lately, unless that leaves nothing but the one showing now.
function fresh(quips: string[]): string[] {
	const unseen = quips.filter((quip) => !recent.includes(quip));
	return unseen.length > 0 ? unseen : quips.filter((quip) => quip !== recent.at(-1));
}

function remember(quip: string): string {
	recent.push(quip);
	if (recent.length > 50) recent.shift();
	return quip;
}

/**
 * A quip for the greeting, usually the most fitting kind there is: a running game, then the
 * occasion, your situation, the patch and the moment, then everyday ones. The last 50 picks sit
 * out, so rerolls don't come back around quickly.
 * @param ctx - What the quip may use.
 * @returns The quip.
 */
export function pickQuip(ctx: GreetingContext): string {
	const fits = fitsPatch(ctx.instance?.version);
	const tiers: [string[], number][] = ctx.running
		? [[pool("home_quip_running"), 1]]
		: [
				[occasionQuips(ctx).filter(fits), 0.6],
				[situationQuips(ctx).filter(fits), 0.6],
				[patchQuips(ctx, fits), 0.25],
				[momentQuips(ctx).filter(fits), 0.25],
				[everydayQuips(ctx).filter(fits), 1],
			];
	const left = tiers
		.map(([quips, chance]) => [fresh(quips), chance] as const)
		.filter(([quips]) => quips.length > 0);
	for (const [quips, chance] of left.slice(0, -1)) {
		if (Math.random() < chance) return remember(pick(quips)!);
	}
	return remember(pick(left.at(-1)?.[0] ?? []) ?? "");
}

/**
 * What the greeting says when the falling weather is switched on or off.
 * @param weather - The weather now falling, or `off`.
 * @returns The quip.
 */
export function weatherQuip(weather: Weather | "off"): string {
	return pick(pool(`home_quip_weather_${weather}`)) ?? "";
}

/**
 * The quip for rerolling exactly this many times, if the count is a milestone.
 * @param count - Rerolls so far.
 * @returns The quip, or undefined between milestones.
 */
export function rerollQuip(count: number): string | undefined {
	return messages[`home_quip_reroll_${count}`]?.();
}
