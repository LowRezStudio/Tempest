export type Tone = "info" | "success" | "warning" | "error";
export type Toast = { id: number; tone: Tone; title?: string; message: string };

let nextId = 1;

class Toasts {
	items = $state<Toast[]>([]);

	push(tone: Tone, message: string, title?: string, ms = tone === "error" ? 9000 : 4000) {
		const id = nextId++;
		this.items.push({ id, tone, title, message });
		setTimeout(() => this.remove(id), ms);
		return id;
	}

	remove(id: number) {
		this.items = this.items.filter((t) => t.id !== id);
	}

	error(error: unknown, title = "Error") {
		const message = error instanceof Error ? error.message : String(error);
		console.error(title, error);
		return this.push("error", message, title);
	}
}

export const toasts = new Toasts();
