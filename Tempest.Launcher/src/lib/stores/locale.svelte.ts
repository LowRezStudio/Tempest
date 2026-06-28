import { getLocale, setLocale, type Locale } from "$lib/paraglide/runtime";

class LocaleState {
	#current = $state<Locale>(getLocale());

	get current() {
		return this.#current;
	}

	set(newLocale: Locale) {
		if (newLocale === this.#current) return;
		this.#current = newLocale;
		void setLocale(newLocale, { reload: false });
	}
}

export const localeState = new LocaleState();
