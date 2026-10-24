import type { Settings } from "@lucide/svelte";

export type PageSection = {
	id: string;
	title: string;
	icon: typeof Settings;
	tone?: "danger";
};

/** Follows which Panel of a long page is in view, for the page's SectionNav. */
export class SectionTracker {
	current = $state("");
	/** The panel a nav click went to, lit up for a moment so the eye lands on it */
	flashed = $state<string | null>(null);

	readonly #sections: () => PageSection[];
	// While a nav click scrolls, the click decides which section is current, not the ones passing by.
	#jumping = false;
	#flashTimer: ReturnType<typeof setTimeout> | undefined;

	constructor(sections: () => PageSection[]) {
		this.#sections = sections;
		this.current = sections()[0]?.id ?? "";
	}

	get sections() {
		return this.#sections();
	}

	/**
	 * @param id - The section's id.
	 * @returns The props of the Panel showing it.
	 */
	panel = (id: string) => ({
		...this.sections.find((s) => s.id === id)!,
		highlighted: this.flashed === id,
	});

	jump = (id: string) => {
		this.current = id;
		this.#jumping = true;
		this.flashed = id;
		clearTimeout(this.#flashTimer);
		this.#flashTimer = setTimeout(() => (this.flashed = null), 1200);
		document.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
	};

	/**
	 * @param event - The scroll container's scroll event.
	 */
	track = (event: Event) => {
		const scroller = event.currentTarget as HTMLElement;
		const sections = this.sections;
		if (this.#jumping || sections.length === 0) return;
		const atBottom = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4;
		if (atBottom) {
			this.current = sections.at(-1)!.id;
			return;
		}
		const line = scroller.getBoundingClientRect().top + 80;
		let id = sections[0]!.id;
		for (const s of sections) {
			const top = document.querySelector(`#${s.id}`)?.getBoundingClientRect().top;
			if (top !== undefined && top <= line) id = s.id;
		}
		this.current = id;
	};

	/** For the scroll container's scrollend and wheel events: the jump is over, or the user took over */
	settle = () => {
		this.#jumping = false;
	};
}
