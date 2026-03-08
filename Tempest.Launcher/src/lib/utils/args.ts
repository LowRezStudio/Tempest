export function parseArgs(input: string): string[] {
	return input
		.split(/\s+/)
		.map(arg => arg.trim())
		.filter(arg => arg.length > 0);
}