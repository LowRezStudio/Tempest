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
	imports: ImportRow[];
	exports: ExportRow[];
};

/** The export table as the backend sends it: one column per field (the
 * renderer builds the `ExportRow`s, their paths and class names). */
export type ExportColumns = {
	names: string[];
	classes: number[];
	outers: number[];
	supers: number[];
	archetypes: number[];
	sizes: number[];
	offsets: number[];
	exportFlags: number[];
	/** Object flags as hex. */
	objectFlags: string[];
};
export type PackageTablesWire = Omit<PackageTables, "exports"> & { exports: ExportColumns };

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
	/** GLB bytes (previews only; the host decodes the server's base64). */
	glb?: Uint8Array;
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
			ogg: Uint8Array;
			bytes: number;
			channels: number | null;
			sampleRate: number | null;
			duration: number | null;
	  }
	| { kind: "swf"; bytes: number; signature: string };

/** One Wwise media file (in a bank or streamed). */
export type WwiseMedia = {
	id: number;
	bytes: number;
	codec: string;
	channels: number;
	sampleRate: number;
	duration: number;
	streamed: boolean;
	bank: number | null;
	hash?: string;
	error?: string;
};

export type WwiseEvent = { id: number; name: string | null; media: number[] };

export type WwiseBank = {
	id: number;
	name: string | null;
	version?: number;
	objects?: number;
	error?: string;
	media: WwiseMedia[];
	events: WwiseEvent[];
};

/** A `.pck` file package. */
export type WwisePackage = {
	path: string;
	name: string;
	languages: string[];
	banks: WwiseBank[];
	streams: WwiseMedia[];
};

/** What an AkEvent / AkBank export resolves to. */
export type WwiseObject = {
	pck: string | null;
	bank: string;
	bankId: number;
	eventId: number | null;
	name: string;
	found?: boolean;
	media: WwiseMedia[];
	trail: { kind: string; id: number }[];
};

/** Edited PCM sent back to the backend (interleaved 16-bit, base64). */
export type PcmPayload = { pcm: string; channels: number; sampleRate: number; quality?: number };

/** A cooked particle system for the preview: every object as its parsed view. */
export type ParticleSystemJson = {
	root: number;
	system: ParsedObject;
	emitters: {
		index: number;
		path: string;
		class: string;
		props: ParsedObject;
		lods: {
			index: number;
			props: ParsedObject;
			required: ParsedObject | null;
			spawn: ParsedObject | null;
			typeData: ParsedObject | null;
			modules: ParsedObject[];
		}[];
	}[];
	materials: Record<
		string,
		{
			blendMode: string | null;
			twoSided: boolean | null;
			png: Uint8Array | null;
			texture: string | null;
			color: number[] | null;
		}
	>;
	meshes: Record<string, { path: string; index: number }>;
};

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

export type ReplaceMatch = {
	index: number;
	object: string;
	/** Property path, or null for the object's own name. */
	prop: string | null;
	kind: "str" | "name" | "enum" | "object" | "objectname";
	current: string;
	replacement: string;
};

export type BatchEntry = {
	file: string;
	name: string;
	package: string;
	packagePath: string | null;
	object: string | null;
	index: number | null;
	kind: string | null;
	ok: boolean;
	message: string;
};

export type BatchResult = { entries: BatchEntry[]; ok: number; failed: number; ran: boolean };

export type MaterialPreview = {
	chain: { package: string; object: string; class: string }[];
	textures: {
		parameter: string;
		slot: "basecolor" | "normal" | "specular" | "emissive" | "mask" | "other";
		path: string;
		package: string;
		export: number;
		png: Uint8Array | null;
		error: string | null;
	}[];
	scalars: { name: string; value: number }[];
	vectors: { name: string; value: [number, number, number, number] }[];
	switches: { name: string; value: boolean }[];
};

type Target = { path: string; index: number };

/** Every `upk serve` method with its params and result. */
/** A column of a marshal row set: present in `count` of its rows. */
export type McsColumn = { name: string; token: number; kind: McsKind; count: number };
export type McsKind =
	| "unknown"
	| "byte"
	| "int"
	| "word"
	| "dword"
	| "float"
	| "double"
	| "qword"
	| "netid"
	| "datetime"
	| "string"
	| "rowset"
	| "guid"
	| "blob";
export type McsTable = { name: string; token: number; rows: number; columns: McsColumn[] };
export type McsHeaderField = { name: string; token: number; kind: McsKind; value: McsCell };
/** A scalar cell as JSON; 64-bit numbers travel as decimal strings; row sets as `{rows}`. */
export type McsCell = number | string | { rows: number } | null;
export type McsDoc = {
	id: string;
	path: string;
	function: string;
	flags: number;
	dirty: boolean;
	undo: string[];
	header: McsHeaderField[];
	tables: McsTable[];
};
export type McsSummary = { tokens: string; fields: number; docs: McsDoc[] };
/** A row set address: field names and row indices from the document root. */
export type McsPath = [string, number][];

/** Linked: deltas against the game folder (small); portable: whole files (opens anywhere). */
export type TwrkMode = "linked" | "portable";
export type TwrkFile = {
	kind: "upk" | "mcts";
	path: string;
	rel: string | null;
	stored: "delta" | "full";
	entry: string;
	bytes: number;
	doc?: string;
	unchanged?: boolean;
};
export type McsRowsPage = {
	total: number;
	offset: number;
	rows: [number, Record<string, McsCell>][];
};
export type McsField = { name: string; token: number; kind: McsKind; value: McsCell };
export type McsStatus = { doc: string; dirty: boolean; undo: string[] };
export type McsMessage = { text: string; name: string; path: McsPath };
export type McsValidValue = { group: number; msgId: number; text: string };

export type SwfTag = {
	index: number;
	code: number;
	name: string;
	len: number;
	id: number | null;
	detail: string;
};
export type SwfImage = {
	tag: number;
	id: number;
	format: number;
	width: number;
	height: number;
	exportName: string;
	fileName: string;
};
export type SwfText = {
	tag: number;
	id: number;
	variable: string;
	text: string | null;
	fontId: number | null;
	fontClass: string | null;
	fontHeight: number | null;
	html: boolean;
	multiline: boolean;
};
export type SwfScript = {
	tag: number;
	kind: string;
	sprite: number | null;
	bytes: number;
	strings: { index: number; text: string; action: string }[];
};
export type SwfInfo = {
	signature: string;
	version: number;
	compressed: boolean;
	width: number;
	height: number;
	frameRate: number;
	frames: number;
	tags: SwfTag[];
	images: SwfImage[];
	texts: SwfText[];
	scripts: SwfScript[];
	exports: { tag: number; id: number; name: string }[];
	imports: { tag: number; url: string; symbols: { id: number; name: string }[] }[];
};
export type SwfEdit = {
	kind: "text" | "string" | "image";
	tag: number;
	index?: number;
	text: string;
};

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
	/** The name table of an open package. */
	"package.names": { params: { path: string }; result: string[] };
	/** One export row in full (path, flag names). */
	"package.export": { params: { path: string; index: number }; result: ExportRow };
	"package.close": { params: { path: string }; result: boolean };
	"package.dirty": { params: { path: string }; result: boolean };
	"package.save": {
		params: { path: string; out?: string };
		result: {
			out: string;
			reopened: boolean;
			dirty: boolean;
			undo: string[];
			dbReloaded?: boolean;
		};
	};
	"package.rewrite": {
		params: { path: string; out: string; compress: boolean; obscure?: boolean };
		result: { out: string; compress: boolean; obscure: boolean };
	};
	"package.verify": { params: { path: string }; result: VerifyReport };
	"package.undo": {
		params: { path: string };
		result: { label: string | null; remaining: string[]; dirty: boolean };
	};
	"package.undoList": { params: { path: string }; result: { labels: string[]; dirty: boolean } };
	"package.discard": { params: { path: string }; result: boolean };
	"object.get": { params: Target; result: ParsedObject };
	"object.text": { params: Target; result: string };
	"object.set": {
		params: Target & { prop: string; value: string };
		result: ParsedObject;
	};
	/** Add / duplicate / remove an array element, or reset a property to its class default. */
	"object.list": {
		params: Target & {
			prop: string;
			op: "add" | "duplicate" | "remove" | "removeProperty";
			at?: number;
		};
		result: ParsedObject;
	};
	"object.rename": { params: Target & { name: string }; result: ExportRow };
	"object.delete": { params: { path: string; indices: number[] }; result: { cleared: number } };
	"object.copy": {
		params: { src: string; index: number; dst: string; outer?: number | "top"; deep?: boolean };
		result: { index: number; path: string; warnings: string[] };
	};
	/** A copy of an export next to it (`…_copy`), same payload, no sub-objects. */
	"object.duplicate": { params: Target; result: { index: number; path: string } };
	"object.deps": { params: Target; result: Deps };
	"object.extract": { params: Target & { out: string }; result: { out: string; bytes: number } };
	"texture.info": { params: Target; result: TextureInfo };
	"texture.preview": {
		params: Target & { mip?: number };
		result: { png: Uint8Array; bytes: number };
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
		params: Target & { lod?: number; textures?: boolean; skin?: boolean; textureSize?: number };
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
	"media.import": {
		params: Target & { file: string; quality?: number };
		result: { kind: string; bytes: number };
	};
	"sound.replacePcm": {
		params: Target & PcmPayload;
		result: { bytes: number; duration: number; channels: number; sampleRate: number };
	};
	"wwise.packages": {
		params: Record<string, never>;
		result: { packages: { name: string; path: string; size: number }[] };
	};
	"wwise.open": { params: { path: string }; result: WwisePackage };
	"wwise.preview": {
		params: { path: string; id: number };
		result: WwiseMedia & { ogg: Uint8Array };
	};
	"wwise.export": {
		params: { path: string; id: number; out: string };
		result: { out: string; bytes: number };
	};
	"wwise.import": {
		params: { path: string; id: number; file: string; quality?: number; out?: string };
		result: { out: string; bytes: number };
	};
	"wwise.replacePcm": {
		params: { path: string; id: number; out?: string } & PcmPayload;
		result: { out: string; bytes: number };
	};
	"wwise.forObject": { params: Target; result: WwiseObject };
	/** Open assembly.dat and the Lang_*.dat files of a cooked folder (the workspace by default). */
	"mcts.open": { params: { dir?: string; reload?: boolean }; result: McsSummary };
	"mcts.summary": { params: Record<string, never>; result: McsSummary };
	"mcts.doc": { params: { doc: string }; result: McsDoc };
	"mcts.table": { params: { doc: string; path?: McsPath; table: string }; result: McsTable };
	"mcts.rows": {
		params: {
			doc: string;
			path?: McsPath;
			table: string;
			offset?: number;
			limit?: number;
			filter?: string;
			sort?: string;
			desc?: boolean;
		};
		result: McsRowsPage;
	};
	"mcts.row": { params: { doc: string; path: McsPath }; result: { fields: McsField[] } };
	"mcts.rowFull": { params: { doc: string; path: McsPath }; result: Record<string, unknown> };
	/** Set a field of a row from typed text (`text`) or JSON (`value`, null removes it). */
	"mcts.set": {
		params: { doc: string; path: McsPath; field: string; text?: string; value?: unknown };
		result: McsStatus & { row: Record<string, McsCell> };
	};
	"mcts.setHeader": {
		params: { doc: string; field: string; text?: string; value?: unknown };
		result: McsStatus;
	};
	"mcts.insert": {
		params: { doc: string; path?: McsPath; table: string; index?: number; clone?: number };
		result: McsStatus & { index: number };
	};
	/**
	 * Append a row at the end of a row set: empty (the last row's shape) or a copy of `clone`;
	 * the set's id column gets a fresh id, nested sets are renumbered and, with `messages`
	 * (default true), the copy's `*_MSG_ID`s get new messages with the same texts.
	 */
	"mcts.addRow": {
		params: {
			doc: string;
			path?: McsPath;
			table: string;
			clone?: number;
			/** A row as JSON (paste): used instead of `clone`. */
			row?: Record<string, unknown>;
			messages?: boolean;
		};
		result: McsStatus & {
			index: number;
			key: string | null;
			id: number | null;
			messages: { field: string; from: number; to: number }[];
		};
	};
	/** Rows that refer to an id (same id family: BOT_ID, REF_BOT_ID…), at most 500. */
	"mcts.usages": {
		params: { doc?: string; field: string; value: number };
		result: { usages: { path: McsPath; field: string; label: string }[]; truncated: boolean };
	};
	"mcts.delete": { params: { doc: string; path: McsPath }; result: McsStatus };
	"mcts.undo": { params: { doc: string }; result: McsStatus & { undone: string | null } };
	"mcts.save": { params: { doc: string; out?: string }; result: McsStatus & { out: string } };
	"mcts.discard": { params: { doc: string }; result: McsStatus };
	"mcts.find": {
		params: { doc: string; table: string; key: string; value: number | string };
		result: number[];
	};
	"mcts.messages": {
		params: { ids: number[]; langs?: string[] };
		result: Record<string, Record<string, McsMessage>>;
	};
	"mcts.setMessage": {
		params: { lang: string; id: number; text: string; name?: string };
		result: McsStatus;
	};
	"mcts.validValues": { params: Record<string, never>; result: Record<string, McsValidValue> };
	/** The next free ids of every kind (items and devices share one space). */
	"mcts.nextIds": {
		params: Record<string, never>;
		result: {
			item: number;
			device: number;
			bot: number;
			res: number;
			icon: number;
			fx: number;
			msg: number;
			deviceMode: number;
			skin: number;
			/** The id range in force (`mcts.idRange`), when one is set. */
			range: { start: number; end: number } | null;
		};
	};
	/** Clone a device/card (DEVICE_ID), item (ITEM_ID) or champion (BOT_ID) with fresh ids and new messages. */
	"mcts.create": {
		params: {
			kind: "device" | "card" | "item" | "champion";
			template: number;
			name: string;
			description?: string;
			shortDescription?: string;
			bot?: number;
			slot?: number;
			itemType?: number;
		};
		result: {
			kind: string;
			deviceId?: number;
			itemId?: number;
			botId?: number;
			messages: number[];
			device?: McsPath;
			item?: McsPath | null;
			bot?: McsPath;
			skin?: McsPath | null;
			devices?: unknown[];
			items?: unknown[];
		};
	};
	/** The Champion Vendor loot table (475, what the client lists as champions) against the champion bots. */
	"mcts.vendor": {
		params: Record<string, never>;
		result: {
			table: number;
			/** Row index of the loot table in DATA_SET_LOOT_TABLES, null when the table is absent. */
			index: number | null;
			total: number;
			active: number;
			listed: { bot: number | null; item: number; active: boolean }[];
			missing: { bot: number; item: number; reference: string; name: string }[];
		};
	};
	/** Add a champion's Character Class item to the Champion Vendor. */
	"mcts.vendorAdd": {
		params: { bot: number };
		result: { bot: number; item: number; vendor: McsPath | null; existed: boolean };
	};
	/** The GET_PROFILE_CLASSES row and inventory items of a champion, for the private server. */
	"mcts.serverRows": {
		params: { bot: number };
		result: { profileClass: Record<string, unknown>; inventory: unknown[] };
	};
	/** Write a mod folder: manifest, changed .dat files, paserver.json, Lua + UnrealScript scaffold. */
	"mcts.scaffold": {
		params: {
			dir: string;
			id: string;
			name?: string;
			version?: string;
			author?: string;
			bot?: number;
			devices?: number[];
			allLanguages?: boolean;
			package?: string;
			/** Ship the .dat files whose base is recorded as row deltas (default true). */
			deltas?: boolean;
		};
		result: {
			dir: string;
			files: string[];
			deltas: { file: string; ops: number; bytes: number }[];
			classes: string[];
			package: string;
		};
	};
	/** New ids come from `start..end` (a per-mod range so two Studio-made mods never collide); both absent clears it. */
	"mcts.idRange": {
		params: { start?: number; end?: number };
		result: UpkMethods["mcts.nextIds"]["result"];
	};
	"mcts.close": { params: Record<string, never>; result: boolean };
	/** Tags, texts, scripts and external images of a Scaleform movie export. */
	"swf.info": { params: { path: string; index: number }; result: SwfInfo };
	/** ActionScript 2 disassembly of one script tag. */
	"swf.script": {
		params: { path: string; index: number; tag: number };
		result: { listing: string };
	};
	/** Write edits (edit-text initial text, script strings, external image names) back into the package. */
	"swf.apply": {
		params: { path: string; index: number; edits: SwfEdit[] };
		result: { edits: number; bytes: number; undo: string[] };
	};
	"particle.system": { params: Target & { textureSize?: number }; result: ParticleSystemJson };
	"particle.setTable": {
		params: Target & {
			prop: string;
			table: number[];
			timeScale?: number;
			startTime?: number;
			op?: number;
			numElements?: number;
			chunkSize?: number;
		};
		result: ParsedObject;
	};
	"script.source": { params: Target; result: ScriptSource };
	"script.compile": {
		params: { path: string; class: string; source: string };
		result: PackageTables & { warnings: string[]; compileMs: number };
	};
	"script.export": {
		params: { path: string; outDir: string };
		result: { dir: string; classes: number };
	};
	"material.preview": { params: Target & { textureSize?: number }; result: MaterialPreview };
	"material.setParameter": {
		params: Target & {
			name: string;
			scalar?: number;
			vector?: [number, number, number, number];
			textureSize?: number;
		};
		result: MaterialPreview & { changed: number };
	};
	"package.findReplace": {
		params: {
			path: string;
			query: string;
			replacement?: string;
			names?: boolean;
			apply?: boolean;
		};
		result: {
			matches: ReplaceMatch[];
			applied: number;
			errors: { index: number; object: string; prop: string | null; error: string }[];
			dirty: boolean;
		};
	};
	/** Write the session's unsaved packages and marshal files to a workspace file. */
	"twrk.save": {
		params: { out: string; mode: TwrkMode; state: unknown; include?: string[] };
		result: {
			out: string;
			mode: TwrkMode;
			files: TwrkFile[];
			size: number;
			warnings: string[];
		};
	};
	/** A workspace file's manifest, without applying it. */
	"twrk.read": {
		params: { path: string };
		result: {
			format: number;
			mode: TwrkMode;
			gameDir: string;
			saved: number;
			files: TwrkFile[];
			state: unknown;
		};
	};
	/** Put a workspace file's changes into the open workspace (all unsaved afterwards). */
	"twrk.apply": {
		params: { path: string };
		result: {
			packages: { path: string; stored: "delta" | "full"; changes: number; dirty: boolean }[];
			docs: string[];
			extras: string[];
			problems: string[];
			state: unknown;
			mode: TwrkMode;
		};
	};
	/** What a mod made from this workspace could hold (unsaved edits, files saved since their base). */
	"mod.changes": {
		params: Record<string, never>;
		result: {
			files: {
				path: string;
				kind: "upk" | "mcts";
				doc?: string;
				state: "unsaved" | "saved";
				rel: string;
				base: boolean;
				script: boolean;
			}[];
		};
	};
	/** Write a launcher mod folder (manifest, README, icon, deltas/files, extra files). */
	"mod.cook": {
		params: {
			dir: string;
			id: string;
			name: string;
			version: string;
			authors: { name: string; link: string }[];
			readme?: string;
			dependencies?: string[];
			icon?: string;
			items: { path: string; kind: "upk" | "mcts"; doc?: string; mode: "delta" | "whole" }[];
			extras: { src: string; dest: string }[];
		};
		result: {
			dir: string;
			entries: {
				file: string;
				entry?: string;
				mode: "delta" | "whole" | "unchanged" | "extra";
				ops?: number;
				warnings?: string[];
			}[];
		};
	};
	"batch.plan": { params: { dir: string; recursive?: boolean }; result: BatchResult };
	"batch.run": {
		/** `packages`: the package files (paths from the plan) to write; every touched one when absent. */
		params: { dir: string; recursive?: boolean; outDir?: string; packages?: string[] };
		result: BatchResult;
	};
	"mod.dir": {
		params: {
			dir: string;
			id: string;
			name?: string;
			version?: string;
			author?: string;
			files: string[];
			/** Ship files whose base is recorded as deltas (default true); false copies every file whole. */
			deltas?: boolean;
		};
		result: { dir: string; files: string[]; entries: ModDirEntry[] };
	};
	/** Is the file's base (its state before the first in-place save) recorded, so a delta can be made? */
	"delta.status": { params: { path: string }; result: DeltaStatus };
	/** Take the file as it is now as the base (replacing any earlier record). */
	"delta.record": { params: { path: string }; result: DeltaStatus & { ms: number } };
	"delta.reset": { params: { path: string }; result: boolean };
	"delta.info": {
		params: { file: string };
		result: { kind: string; target: string; header: Record<string, unknown> };
	};
	/** One delta file from a game file and its recorded base. */
	"delta.create": {
		params: { path: string; out: string; target?: string };
		result: {
			out: string;
			target: string;
			kind: string;
			ops: { op: string; path: string; class: string }[];
			tables: unknown;
			bytes: number;
			warnings: string[];
		};
	};
}

/** How one file went into a mod folder (`mod.dir`). */
export interface ModDirEntry {
	/** Game-relative path (`ChaosGame/CookedPCConsole/Foo.upk`). */
	file: string;
	/** `delta`: only what changed; `copy`: the whole file (no base recorded); `unchanged`: skipped. */
	mode: "delta" | "copy" | "unchanged";
	ops: number;
	bytes: number;
	warnings: string[];
}

export interface DeltaStatus {
	recorded: boolean;
	/** Unix seconds when the base was recorded. */
	when?: number;
	size?: number;
	objects?: number;
	target?: string;
}

export type UpkMethod = keyof UpkMethods;

/** Status of the backend process. */
export type McpInfo = { url: string; token: string; configPath: string; running: boolean };

export type UpkStatus = {
	running: boolean;
	binary: string | null;
	pid: number | null;
	error: string | null;
};

/** An event line pushed by `upk serve` (no request id). */
export type UpkEvent = { event: string; [key: string]: unknown };

/** A framed response as the host hands it to the renderer: the result JSON
 * bytes (or an already parsed `value` for unframed replies) and the binary
 * payloads referenced by `{"$bin": n}` markers inside it. */
export type UpkFrame = { json: Uint8Array | null; value?: unknown; bins: Uint8Array[] };

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
/** What a new window opens once its renderer is up (pulled through `window:startup`). */
export type WindowStartup = { dir?: string; files?: string[] };

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
	/** Open another Studio window (with its own backend), optionally on a workspace or packages. */
	"window:new": { args: WindowStartup; result: void };
	/** What this window was asked to open when it was created; `done` clears it once applied. */
	"window:startup": { args: { done?: boolean }; result: WindowStartup | null };

	/**
	 * The crash-recovery workspace file of a game folder (`<userData>/recovery/<hash>.twrk`):
	 * its path and, when it exists, its modification time; `clear` deletes it.
	 */
	"workspace:recovery": {
		args: { dir: string; clear?: boolean };
		result: { path: string; exists: boolean; mtime: number | null };
	};

	/** tempest-cli for the mod cooker: where it is and whether a signing key exists. */
	"cli:status": {
		args: Record<string, never>;
		result: { path: string | null; available: boolean; signingKey: string; hasKey: boolean };
	};
	/** Run `tempest-cli mod <args>` (pack, sign, verify, install, publish, login, gen-key…). */
	"cli:mod": {
		args: { args: string[] };
		result: { code: number; stdout: string; stderr: string };
	};
	/** A fresh staging folder path for a cooked mod. */
	"cli:staging": { args: { id: string }; result: string };
	/** Zoom the window's content (UI scale), 0.6–2. */
	"window:zoom": { args: { factor: number }; result: void };

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
	/** Where the MCP server listens and the token clients must send. */
	"mcp:info": { args: void; result: McpInfo };
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
