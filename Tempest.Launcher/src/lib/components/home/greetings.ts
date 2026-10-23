// The home page's greeting and the quip under it: by time of day, weekday, season and what the
// launcher knows about you. Mostly jokes.
import { m } from "$lib/paraglide/messages";
import { isAutumn } from "$lib/theme/weather.svelte";

export type GreetingContext = {
	now: Date;
	name: string;
	/** The hero's instance */
	instance?: { label: string; lastPlayedAt?: number; patchName?: string; patchDate?: string };
	running: boolean;
	instanceCount: number;
	downloading: boolean;
};

type Season = "winter" | "christmas" | "newYearsEve" | "newYear" | "halloween" | "aprilFools";

function seasonOf(date: Date): Season | undefined {
	const month = date.getMonth() + 1;
	const day = date.getDate();
	if (month === 12 && day >= 24 && day <= 26) return "christmas";
	if (month === 12 && day === 31) return "newYearsEve";
	if (month === 12) return "winter";
	if (month === 1 && day === 1) return "newYear";
	if (month === 10 && day >= 24) return "halloween";
	if (month === 4 && day === 1) return "aprilFools";
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

const seasonal: Record<Season, () => string[]> = {
	winter: () => [
		m.home_quip_winter_1(),
		m.home_quip_winter_2(),
		m.home_quip_winter_3(),
		m.home_quip_winter_4(),
	],
	christmas: () => [
		m.home_quip_christmas_1(),
		m.home_quip_christmas_2(),
		m.home_quip_christmas_3(),
	],
	newYearsEve: () => [m.home_quip_nye_1(), m.home_quip_nye_2()],
	newYear: () => [m.home_quip_new_year()],
	halloween: () => [
		m.home_quip_halloween_1(),
		m.home_quip_halloween_2(),
		m.home_quip_halloween_3(),
	],
	aprilFools: () => [m.home_quip_april_1(), m.home_quip_april_2()],
};

const DAY = 86_400_000;

// The holiday and the patch's birthday.
function occasionQuips(ctx: GreetingContext): string[] {
	const { now, instance } = ctx;
	const season = seasonOf(now);
	const quips = season ? seasonal[season]() : [];
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

// What the launcher knows about you: downloads, a long break, a museum of patches.
function situationQuips(ctx: GreetingContext): string[] {
	const { now, instance } = ctx;
	const quips: string[] = [];
	if (ctx.instanceCount === 0) quips.push(m.home_quip_no_instances());
	if (ctx.instanceCount >= 5) quips.push(m.home_quip_collector({ count: ctx.instanceCount }));
	if (ctx.downloading) quips.push(m.home_quip_downloading());
	if (instance && !instance.lastPlayedAt) {
		quips.push(m.home_quip_never_played({ name: instance.label }));
	} else if (instance?.lastPlayedAt && now.getTime() - instance.lastPlayedAt > 30 * DAY) {
		const days = Math.floor((now.getTime() - instance.lastPlayedAt) / DAY);
		quips.push(m.home_quip_long_absence(), m.home_quip_days_away({ days }));
	}
	return quips;
}

// What always fits: the time, the weekday, the season, the patch's age and a pile of Paladins jokes.
function everydayQuips(ctx: GreetingContext): string[] {
	const { now, name, instance } = ctx;
	const quips = [
		m.home_quip_1(),
		m.home_quip_2(),
		m.home_quip_3(),
		m.home_quip_4(),
		m.home_quip_5(),
		m.home_quip_6(),
		m.home_quip_7(),
		m.home_quip_8(),
		m.home_quip_9(),
		m.home_quip_10(),
	];
	const hour = now.getHours();
	const weekday = now.getDay();
	if (hour < 5) {
		const time = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
		quips.push(m.home_quip_late_1({ time }), m.home_quip_late_2());
	} else if (hour < 10) {
		quips.push(m.home_quip_morning());
	}
	if (weekday === 1 && hour < 12) quips.push(m.home_quip_monday());
	if (weekday === 5 && hour >= 17) quips.push(m.home_quip_friday());
	if (weekday === 0 || weekday === 6) quips.push(m.home_quip_weekend());
	if (isAutumn(now)) {
		quips.push(m.home_quip_autumn_1(), m.home_quip_autumn_2(), m.home_quip_autumn_3());
	}
	const year = instance?.patchDate ? new Date(instance.patchDate).getFullYear() : undefined;
	if (year && now.getFullYear() - year >= 5) quips.push(m.home_quip_vintage({ year }));
	if (name === "Player") quips.push(m.home_quip_default_name());
	return quips;
}

const pick = (from: string[]) => from[Math.floor(Math.random() * from.length)];

/**
 * A quip for the greeting, usually the most fitting kind there is: a running game, then the
 * occasion, then your situation, then everyday ones.
 * @param ctx - What the quip may use.
 * @param not - The quip showing now, so a new pick differs from it.
 * @returns The quip.
 */
export function pickQuip(ctx: GreetingContext, not: string | undefined): string {
	const tiers = ctx.running
		? [[m.home_quip_running_1(), m.home_quip_running_2(), m.home_quip_running_3()]]
		: [occasionQuips(ctx), situationQuips(ctx), everydayQuips(ctx)];
	const left = tiers
		.map((tier) => tier.filter((quip) => quip !== not))
		.filter((tier) => tier.length > 0);
	for (const tier of left.slice(0, -1)) {
		if (Math.random() < 0.6) return pick(tier)!;
	}
	return pick(left.at(-1) ?? []) ?? "";
}
