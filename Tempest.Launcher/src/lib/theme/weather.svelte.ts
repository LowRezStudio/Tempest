// What falls over the launcher in each season, and the switches for it: the Seasonal effects
// setting, and the Konami code for everything else.
import { seasonalEffects } from "$lib/stores/settings.svelte";

export type Weather = "snow" | "leaves" | "petals" | "fireflies";

/**
 * Mid-September to November.
 * @param date - The day to check.
 * @returns True in autumn.
 */
export function isAutumn(date: Date): boolean {
	const month = date.getMonth() + 1;
	return (month === 9 && date.getDate() >= 22) || month === 10 || month === 11;
}

type Season = { kind: Weather; auto: boolean; spooky: boolean; festive: boolean };

/**
 * The date's weather: snow in winter, leaves in autumn, petals in spring, fireflies in summer.
 * October is spooky on top (webs and a pumpkin), and
 * December up to Boxing Day festive (a Christmas tree and gifts). Only those stretches show it
 * without asking; the rest of the year it waits for the Konami code.
 * @param date - The day to check.
 * @returns The weather, whether it shows on its own, and the decorations that go with it.
 */
export function weatherOf(date: Date): Season {
	const month = date.getMonth() + 1;
	const day = date.getDate();
	const season = (kind: Weather, auto: boolean): Season => ({
		kind,
		auto,
		spooky: month === 10,
		festive: month === 12 && day <= 26,
	});
	if (month === 12 || (month === 1 && day <= 6)) return season("snow", true);
	if (isAutumn(date)) return season("leaves", true);
	if (month <= 2) return season("snow", false);
	if (month <= 5) return season("petals", false);
	return season("fireflies", false);
}

// A launcher left open in the tray turns over with the seasons.
const clock = $state({ value: new Date() });
setInterval(() => (clock.value = new Date()), 60 * 60 * 1000);

/**
 * Today, kept current while the launcher sits in the tray.
 * @returns The date seasonal things go by.
 */
export function today(): Date {
	return new Date(clock.value);
}

/** This session's say from the Konami code or the home page's prize; null follows the season. */
export const weatherOverride = $state<{ value: boolean | null }>({ value: null });

/** What's showing now: the falling weather and the decorations, if any. */
export const currentWeather = {
	get value(): Season | undefined {
		const season = weatherOf(today());
		const shown = weatherOverride.value ?? (seasonalEffects.value && season.auto);
		return shown ? season : undefined;
	},
};

const konami = [
	"arrowup",
	"arrowup",
	"arrowdown",
	"arrowdown",
	"arrowleft",
	"arrowright",
	"arrowleft",
	"arrowright",
	"b",
	"a",
];
let progress = 0;

/**
 * Feeds a key press to the Konami code; the whole code toggles the weather.
 * @param e - The key press.
 */
export function onKonamiKey(e: KeyboardEvent) {
	const key = e.key.toLowerCase();
	if (key === konami[progress]) progress++;
	// A third up still leaves the last two as the code's start
	else if (key === konami[0]) progress = progress === 2 ? 2 : 1;
	else progress = 0;
	if (progress < konami.length) return;
	progress = 0;
	weatherOverride.value = !currentWeather.value;
}
