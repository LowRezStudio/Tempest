// Single source of truth for the renderer <-> host IPC surface, plus the
// JSON shapes `upk serve` returns (crates/tempest-assets/src/serve.rs and
// json.rs in Tempest.UnrealScript). TYPES ONLY: the renderer imports this
// file with `import type`, so nothing here reaches the bundle.
//
// Channel naming: `<domain>:<verb>` for invoke channels.

// ---- upk serve: shared value types ----

/** `{ ref, path, class }`: an object reference as `json::reference` renders it. */
export type ObjRef = { ref: number; path: string; class: string };

export type PropValue =
	| { t: "bool"; v: boolean }
	| { t: "byte"; v: number }
	| { t: "enum"; v: string }
	| { t: "int"; v: number }
	| { t: "float"; v: number }
	| { t: "name"; v: string }
	| { t: "str"; v: string }
	| { t: "object"; v: ObjRef }
	| { t: "delegate"; v: { object: ObjRef; function: string } }
	| { t: "struct"; v: Prop[] }
	| { t: "binary"; v: BinaryMember[] }
	| { t: "array"; v: PropValue[] }
	| { t: "raw"; v: { len: number; hex: string } };

export type Prop = {
	name: string;
	index: number;
	type: string;
	struct?: string;
	enum?: string;
	value: PropValue;
};

export type BinaryMember = { name: string; type: string; elems: PropValue[] };

export type ExportRow = {
	i: number;
	name: string;
	path: string;
	class: string;
	classRef: number;
	outer: number;
	super: number;
	archetype: number;
	size: number;
	offset: number;
	flags: string;
	flagsHex: string;
	exportFlags: number;
};

export type ImportRow = {
	i: number;
	name: string;
	path: string;
	class: string;
	classPackage: string;
	outer: number;
};

export type PackageSummary = {
	fileVersion: number;
	licenseeVersion: number;
	engineVersion: number;
	cookedContentVersion: number;
	packageFlags: number;
	packageFlagsText: string;
	compressionFlags: number;
	compressed: boolean;
	obscured: boolean;
	cooked: boolean;
	folder: string;
	guid: string;
	chunks: number;
	headerSize: number;
	names: number;
	imports: number;
	exports: number;
	additionalPackages: string[];
	textureAllocations: number;
};

export type PackageTables = {
	name: string;
	path: string;
	dirty: boolean;
	loadMs?: number;
	summary: PackageSummary;
	names: string[];
	imports: ImportRow[];
	exports: ExportRow[];
};

export type WorkspacePackage = {
	name: string;
	path: string;
	size: number;
	script: boolean;
	map: boolean;
	open: boolean;
};

export type Workspace = {
	dir: string;
	scriptPackages: string[];
	textureIndex: boolean;
	loadMs: number;
	packages: WorkspacePackage[];
};

export type SearchHit = {
	package: string;
	path: string;
	index: number;
	object: string;
	class: string;
};

export type NativeInfo = { type: string; size: number; text: string };

export type ParsedObject = {
	index: number;
	path: string;
	class: string;
	row: ExportRow;
	dirty?: boolean;
} & (
	| { kind: "empty" }
	| { kind: "raw"; error: string; size: number; hex: string }
	| { kind: "script"; netIndex: number; text: string }
	| {
			kind: "object";
			netIndex: number;
			isClass: boolean;
			knownClass: boolean;
			stateFrame?: {
				node: ObjRef;
				state: ObjRef;
				probeMask: number;
				latentAction: number;
				stack: number;
				codeOffset: number | null;
			};
			component?: { templateOwnerClass: ObjRef; templateName: string | null };
			nativePrefix?: number;
			properties: Prop[];
			native: NativeInfo;
			hasRawValues: boolean;
	  }
);

export type MipLocation =
	| { kind: "inline" }
	| { kind: "unused" }
	| { kind: "missing" }
	| { kind: "cache"; file: string; offset: number; sizeOnDisk: number; size: number };

export type TextureInfo = {
	path: string;
	format: string;
	supported: boolean;
	width: number;
	height: number;
	lightmap: boolean;
	mips: {
		index: number;
		width: number;
		height: number;
		location: MipLocation;
		storedBytes: number;
	}[];
};

export type MeshInfo = {
	skeletal: boolean;
	lods: (
		| {
				index: number;
				vertices: number;
				triangles: number;
				bones: number;
				sections: { material: ObjRef; triangles: number }[];
		  }
		| { index: number; error: string }
	)[];
};

export type MeshGlb = {
	glb?: string;
	bytes: number;
	vertices: number;
	triangles: number;
	materials: number;
	images: number;
	warnings: string[];
	out?: string;
};

export type Deps = {
	class: ObjRef;
	outer: ObjRef;
	archetype: ObjRef;
	super: ObjRef;
	references: ObjRef[];
	referrers: ObjRef[];
};

export type VerifyReport = {
	name: string;
	error: string | null;
	exports: number;
	full: number;
	raw: number;
	partialValues: number;
	withTail: number;
	mismatches: number;
	imageIdentical: boolean;
	mismatchSamples: string[];
	rawSamples: string[];
	ok: boolean;
};

export type MediaPreview =
	| {
			kind: "sound";
			ogg: string;
			bytes: number;
			channels: number | null;
			sampleRate: number | null;
			duration: number | null;
	  }
	| { kind: "swf"; bytes: number; signature: string };

/** A decompiled class (the one owning the selected export). */
export type ScriptSource = {
	class: string;
	classIndex: number;
	source: string;
	/** The selected export's name when it is a member (function, state, …) of the class. */
	member: string | null;
	kind: string;
	classes: number;
};

/** Parsed from a `script.compile` rejection (`compile failed: {json}`). */
export type CompileFailure = { message: string; class?: string; line?: number; detail?: string };

type Target = { path: string; index: number };

/** Every `upk serve` method with its params and result. */
export interface UpkMethods {
	ping: { params: Record<string, never>; result: { version: string; pid: number } };
	"workspace.open": { params: { dir: string }; result: Workspace };
	"workspace.packages": { params: Record<string, never>; result: WorkspacePackage[] };
	"workspace.search": {
		params: { query: string; class?: string; limit?: number };
		result: { results: SearchHit[]; truncated: boolean };
	};
	"package.open": { params: { path: string }; result: PackageTables };
	"package.tables": { params: { path: string }; result: PackageTables };
	"package.close": { params: { path: string }; result: boolean };
	"package.dirty": { params: { path: string }; result: boolean };
	"package.save": {
		params: { path: string; out?: string };
		result: { out: string; reopened: boolean };
	};
	"package.rewrite": {
		params: { path: string; out: string; compress: boolean; obscure?: boolean };
		result: { out: string; compress: boolean; obscure: boolean };
	};
	"package.verify": { params: { path: string }; result: VerifyReport };
	"object.get": { params: Target; result: ParsedObject };
	"object.text": { params: Target; result: string };
	"object.set": { params: Target & { prop: string; value: string }; result: ParsedObject };
	"object.rename": { params: Target & { name: string }; result: ExportRow };
	"object.delete": { params: { path: string; indices: number[] }; result: { cleared: number } };
	"object.copy": {
		params: { src: string; index: number; dst: string; outer?: number | "top"; deep?: boolean };
		result: { index: number; path: string; warnings: string[] };
	};
	"object.deps": { params: Target; result: Deps };
	"object.extract": { params: Target & { out: string }; result: { out: string; bytes: number } };
	"texture.info": { params: Target; result: TextureInfo };
	"texture.preview": {
		params: Target & { mip?: number };
		result: { png: string; bytes: number };
	};
	"texture.export": {
		params: Target & { out: string; mip?: number };
		result: { out: string; bytes: number };
	};
	"texture.exportAll": {
		params: { path: string; outDir: string; png: boolean };
		result: { exported: number; failed: number; errors: string[]; outDir: string };
	};
	"texture.import": {
		params: Target & { file: string; mips?: boolean; format?: string };
		result: TextureInfo;
	};
	"mesh.info": { params: Target; result: MeshInfo };
	"mesh.preview": {
		params: Target & { lod?: number; textures?: boolean; skin?: boolean };
		result: MeshGlb;
	};
	"mesh.export": {
		params: Target & { out: string; lod?: number; textures?: boolean; skin?: boolean };
		result: MeshGlb;
	};
	"mesh.import": {
		params: Target & { file: string; lod?: number };
		result: MeshInfo & {
			vertices: number;
			triangles: number;
			sections: number;
			chunks: number;
			warnings: string[];
		};
	};
	"object.replaceRaw": {
		params: Target & { file: string };
		result: { bytes: number; kind: string; error: string | null };
	};
	"media.preview": { params: Target; result: MediaPreview };
	"media.export": {
		params: Target & { out: string };
		result: { out: string; bytes: number; kind: string };
	};
	"media.import": { params: Target & { file: string }; result: { kind: string; bytes: number } };
	"script.source": { params: Target; result: ScriptSource };
	"script.compile": {
		params: { path: string; class: string; source: string };
		result: PackageTables & { warnings: string[]; compileMs: number };
	};
	"script.export": {
		params: { path: string; outDir: string };
		result: { dir: string; classes: number };
	};
	"mod.dir": {
		params: {
			dir: string;
			id: string;
			name?: string;
			version?: string;
			author?: string;
			files: string[];
		};
		result: { dir: string; files: string[] };
	};
}

export type UpkMethod = keyof UpkMethods;

/** Status of the backend process. */
export type UpkStatus = {
	running: boolean;
	binary: string | null;
	pid: number | null;
	error: string | null;
};

/** An event line pushed by `upk serve` (no request id). */
export type UpkEvent = { event: string; [key: string]: unknown };

// ---- Dialogs ----

export type DialogFilter = { name: string; extensions: string[] };
export type OpenDialogOptions = {
	directory?: boolean;
	multiple?: boolean;
	filters?: DialogFilter[];
	title?: string;
	defaultPath?: string;
};
export type SaveDialogOptions = {
	defaultPath?: string;
	filters?: DialogFilter[];
	title?: string;
};

/** OS info injected by preload (see `window.__os`). */
export type OsInfo = {
	platform: "windows" | "macos" | "linux";
	arch: string;
};

/**
 * The typed invoke surface. `args: void` channels take no payload.
 * Handlers may reject; the renderer sees the rejection as an `Error`.
 */
export interface IpcContract {
	"app:version": { args: void; result: string };
	"app:home": { args: void; result: string };
	"app:temp-path": { args: { name: string }; result: string };

	"window:set-title": { args: { title: string }; result: void };

	"dialog:open": { args: OpenDialogOptions; result: string[] | null };
	"dialog:save": { args: SaveDialogOptions; result: string | null };

	"fs:exists": { args: { path: string }; result: boolean };
	"fs:read-text": { args: { path: string }; result: string };
	"fs:write-text": { args: { path: string; text: string }; result: void };
	"fs:remove": { args: { path: string }; result: void };

	"shell:show-item": { args: { path: string }; result: void };
	"shell:open-path": { args: { path: string }; result: string };

	/** One `upk serve` call. Rejects with the server's error text. */
	"upk:call": { args: { method: UpkMethod; params: unknown }; result: unknown };
	"upk:status": { args: void; result: UpkStatus };
	"upk:restart": { args: void; result: UpkStatus };
}

export type IpcChannel = Extract<keyof IpcContract, string>;

/** Events the host pushes to the renderer. */
export interface IpcEvents {
	/** A progress or notification line from `upk serve`. */
	"upk:event": UpkEvent;
	/** The backend died (crashed or was restarted); open packages are gone. */
	"upk:exit": { code: number | null; signal: string | null };
	/** A .upk file was opened through the OS (file association, argv). */
	"open-files": string[];
}

export type IpcEventChannel = Extract<keyof IpcEvents, string>;

/** Shape of the preload bridge (`window.electronAPI`). */
export type PreloadApi = {
	invoke: <K extends IpcChannel>(
		channel: K,
		...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
	) => Promise<IpcContract[K]["result"]>;
	on: <K extends IpcEventChannel>(
		channel: K,
		callback: (payload: IpcEvents[K]) => void,
	) => () => void;
	getPathForFile: (file: File) => string;
};
