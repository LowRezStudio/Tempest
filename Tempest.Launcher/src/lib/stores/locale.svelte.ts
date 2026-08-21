import { getLocale, setLocale, type Locale } from "$lib/paraglide/runtime";

export const localeLabels: Record<string, { flag: string; label: string }> = {
	en: { flag: "🇬🇧", label: "English" },
	fr: { flag: "🇫🇷", label: "Français" },
	es: { flag: "🇪🇸", label: "Español" },
	pl: { flag: "🇵🇱", label: "Polski" },
	ru: { flag: "🇷🇺", label: "Русский" },
	tr: { flag: "🇹🇷", label: "Türkçe" },
};

class LocaleState {
	#current = $state<Locale>(getLocale());

	get current() {
		return this.#current;
	}

	set current(newLocale: Locale) {
		if (newLocale === this.#current) return;
		this.#current = newLocale;
		void setLocale(newLocale, { reload: false });
	}

	set(newLocale: Locale) {
		this.current = newLocale;
	}
}

export const localeState = new LocaleState();
