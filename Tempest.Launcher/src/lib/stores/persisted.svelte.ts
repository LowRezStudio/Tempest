export interface PersistedOptions<T> {
	encode?: (val: T) => string;
	decode?: (str: string) => T;
}

// One-time import of the settings.json snapshot the Tauri build dumped next to
// this profile (delivered synchronously via preload). Runs at module init, so
// it happens before any persistedJSON()/persistedState() call reads defaults.
// Existing keys are never overwritten.
if (typeof window !== "undefined") {
	const dump = (window as unknown as { __migrationDump?: string | null }).__migrationDump;
	if (dump && !localStorage.getItem("migratedFromTauri")) {
		try {
			const entries = JSON.parse(dump) as Record<string, string>;
			for (const [key, value] of Object.entries(entries)) {
				if (localStorage.getItem(key) === null) localStorage.setItem(key, value);
			}
			// Flag only after the loop completed: on failure (quota, partial write)
			// the next start retries the import. Never overwrite existing keys.
			localStorage.setItem("migratedFromTauri", "1");
		} catch (error) {
			console.warn("[migration] settings.json import skipped:", error);
		}
	}
}

export function persistedState<T>(key: string, defaultValue: T, options?: PersistedOptions<T>) {
	let initial = defaultValue;
	if (typeof window !== "undefined") {
		const stored = localStorage.getItem(key);
		if (stored !== null) {
			if (options?.decode) {
				try {
					initial = options.decode(stored);
				} catch {
					initial = defaultValue;
				}
			} else {
				try {
					initial = JSON.parse(stored);
				} catch {
					initial = stored as T;
				}
			}
		}
	}

	let value = $state<T>(initial);

	return {
		get value() {
			return value;
		},
		set value(newValue: T) {
			value = newValue;
			if (typeof window !== "undefined") {
				if (newValue === undefined) {
					localStorage.removeItem(key);
				} else {
					const encoded = options?.encode
						? options.encode(newValue)
						: typeof newValue === "string"
							? newValue
							: JSON.stringify(newValue);
					localStorage.setItem(key, encoded);
				}
			}
		},
		set(newValue: T) {
			this.value = newValue;
		},
		get() {
			return value;
		},
	};
}

export function persistedJSON<T>(key: string, defaultValue: T) {
	return persistedState<T>(key, defaultValue, {
		encode: JSON.stringify,
		decode: JSON.parse,
	});
}

export function persistedMapState<T extends Record<string, any>>(prefix: string, defaultValue: T) {
	const map = { ...defaultValue };
	if (typeof window !== "undefined") {
		for (let i = 0; i < localStorage.length; i++) {
			const k = localStorage.key(i);
			if (k && k.startsWith(prefix)) {
				const subKey = k.slice(prefix.length);
				const stored = localStorage.getItem(k);
				if (stored !== null) {
					try {
						map[subKey as keyof T] = JSON.parse(stored);
					} catch {
						map[subKey as keyof T] = stored as any;
					}
				}
			}
		}
	}

	let value = $state<T>(map);

	return {
		get value() {
			return value;
		},
		set value(newValue: T) {
			value = newValue;
			if (typeof window !== "undefined") {
				// Remove old keys starting with prefix
				const keysToRemove: string[] = [];
				for (let i = 0; i < localStorage.length; i++) {
					const k = localStorage.key(i);
					if (k && k.startsWith(prefix)) {
						keysToRemove.push(k);
					}
				}
				for (const k of keysToRemove) {
					localStorage.removeItem(k);
				}
				// Write new keys
				for (const [subKey, val] of Object.entries(newValue)) {
					if (val !== undefined) {
						localStorage.setItem(
							prefix + subKey,
							typeof val === "string" ? val : JSON.stringify(val),
						);
					}
				}
			}
		},
		setKey(subKey: string, val: any) {
			if (val === undefined) {
				delete value[subKey];
				if (typeof window !== "undefined") {
					localStorage.removeItem(prefix + subKey);
				}
			} else {
				value[subKey as keyof T] = val;
				if (typeof window !== "undefined") {
					localStorage.setItem(
						prefix + subKey,
						typeof val === "string" ? val : JSON.stringify(val),
					);
				}
			}
		},
		get() {
			return value;
		},
	};
}
