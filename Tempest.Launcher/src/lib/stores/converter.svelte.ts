type FileEntry = { path: string; data: Uint8Array };
type SourceEntry = {
	label: string;
	files: FileEntry[];
	manifest?: {
		name: string;
		id: string;
		version: string;
		authors: { name: string; link: string }[];
	};
	readme?: string;
};

class ConverterStore {
	sources = $state<SourceEntry[]>([]);
	modName = $state("");
	modVersion = $state("1.0.0");
	authors = $state<{ name: string; link: string }[]>([{ name: "", link: "" }]);
	readmeContent = $state("");
	cookedZip = $state<Uint8Array | null>(null);
}

export const converter = new ConverterStore();
