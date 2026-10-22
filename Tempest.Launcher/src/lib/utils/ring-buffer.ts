// Overwrite the oldest entry once capacity is reached to keep scrollback memory bounded.
export class RingBuffer<T> {
	private readonly items: (T | undefined)[];
	private head = 0;
	private count = 0;

	constructor(capacity: number) {
		this.items = Array.from({ length: Math.max(1, Math.floor(capacity)) });
	}

	// A snapshot in oldest-first order.
	toArray(): T[] {
		const items: T[] = [];
		for (let index = 0; index < this.count; index++) {
			items.push(this.items[(this.head + index) % this.items.length]!);
		}
		return items;
	}

	push(value: T): void {
		if (this.count < this.items.length) {
			this.items[(this.head + this.count) % this.items.length] = value;
			this.count++;
			return;
		}
		this.items[this.head] = value;
		this.head = (this.head + 1) % this.items.length;
	}

	clear(): void {
		this.items.fill(undefined);
		this.head = 0;
		this.count = 0;
	}
}
