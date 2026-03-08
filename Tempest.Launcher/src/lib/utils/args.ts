export const parseArgs = (input: string): string[] => {
	const args: string[] = [];
	let current = "";
	let inQuotes = false;

	for (const char of input) {
		if (char === '"' || char === "'") {
			inQuotes = !inQuotes;
			current += char;
		} else if (char === " " && !inQuotes) {
			if (current.trim()) args.push(current.trim());
			current = "";
		} else {
			current += char;
		}
	}
	if (current.trim()) args.push(current.trim());
	return args;
};