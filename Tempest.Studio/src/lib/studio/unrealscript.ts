// CodeMirror language for UnrealScript, on the legacy C-like stream parser.
import { StreamLanguage, type StringStream } from "@codemirror/language";
import { clike } from "@codemirror/legacy-modes/mode/clike";

function words(list: string): Record<string, boolean> {
	const out: Record<string, boolean> = {};
	for (const w of list.split(" ")) out[w] = true;
	return out;
}

const keywords = words(
	"class extends within dependson implements interface native nativereplication noexport abstract " +
		"config perobjectconfig localized transient nontransient placeable notplaceable hidecategories showcategories " +
		"autoexpandcategories collapsecategories dontcollapsecategories hidedropdown editinlinenew noteditinlinenew " +
		"classgroup inherits deprecated nativeonly dllbind forcescriptorder " +
		"var local const struct enum function event state delegate operator preoperator postoperator " +
		"if else for while do until switch case default return break continue goto foreach " +
		"new self super static final simulated exec latent iterator singular private protected public " +
		"auto ignores begin end object defaultproperties structdefaultproperties cpptext structcpptext " +
		"out optional coerce skip init editconst editinline export noclear repnotify databinding " +
		"reliable unreliable replication server client demorecording immutable immutablewhencooked atomic " +
		"editoronly notforconsole duplicatetransient instanced input travel globalconfig editfixedsize " +
		"editconstarray noimport interp repretry crosslevelactive crosslevelpassive archetype",
);
const types = words(
	"int float bool byte name string array map vector rotator object class pointer qword double " +
		"Actor Object Class Vector Rotator Name String",
);
const atoms = words("true false none True False None self Self vect rot");

export const unrealscript = StreamLanguage.define(
	clike({
		name: "unrealscript",
		keywords,
		types,
		atoms,
		blockKeywords: words(
			"if else for while do switch case default foreach state function event",
		),
		hooks: {
			// `macro and `log(...) calls.
			"`": (stream: StringStream) => {
				stream.eatWhile(/\w/);
				return "meta";
			},
			// Name literals: 'Foo'
			"'": (stream: StringStream) => {
				stream.eatWhile(/[^']/);
				stream.eat("'");
				return "atom";
			},
		},
	}),
);
