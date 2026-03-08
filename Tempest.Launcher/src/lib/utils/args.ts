export function parseArgs(input: string): string[] {
	const args: string[] = [];
	let current = "";
	let inQuotes = false;
	let quoteChar = "";

	for (let i = 0; i < input.length; i++) {
		const char = input[i];

		if ((char === '"' || char === "'") && !inQuotes) {
			inQuotes = true;
			quoteChar = char;
		} else if (char === quoteChar && inQuotes) {
			inQuotes = false;
			quoteChar = "";
		} else if (char === " " && !inQuotes) {
			if (current.trim()) {
				args.push(current.trim());
			}
			current = "";
		} else {
			current += char;
		}
	}

	if (current.trim()) {
		args.push(current.trim());
	}

	return args;
}