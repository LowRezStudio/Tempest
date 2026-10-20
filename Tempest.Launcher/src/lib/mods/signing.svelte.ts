// Signing-key UX for the mod cooker: when a package must be signed but this
// machine has no signing key yet, the user picks the key's identity (the name
// shown next to their signature on the repository) in a small dialog.
import {
	SigningKeyMissingError,
	describeError,
	generateSigningKey,
	isKeyExistsError,
	signModPackage,
} from "$lib/core/mods";
import { m } from "$lib/paraglide/messages";

const prompt = $state({
	open: false,
	resolve: null as ((id: string | null) => void) | null,
});

export const signingKeyPrompt = prompt;

/** Opens the key-naming dialog; resolves with the chosen id, or null on cancel. */
export function promptSigningKeyId(): Promise<string | null> {
	return new Promise((resolve) => {
		prompt.open = true;
		prompt.resolve = resolve;
	});
}

export function settleSigningKeyPrompt(id: string | null) {
	prompt.resolve?.(id);
	prompt.resolve = null;
	prompt.open = false;
}

/**
 * Signs the staged package, and when no key exists yet asks the user for a key
 * name first. If a key already exists (generation refuses to overwrite), it is
 * used as-is.
 * @param packagePath Host-side path of the staged .tempest package.
 * @returns The signer description ("user id [fingerprint]").
 */
export async function signWithPrompt(packagePath: string): Promise<string> {
	try {
		return await signModPackage(packagePath);
	} catch (error) {
		if (!(error instanceof SigningKeyMissingError)) throw error;
		const id = await promptSigningKeyId();
		if (!id) throw new Error(m.converter_signing_key_cancelled());
		try {
			await generateSigningKey(id);
		} catch (generateError) {
			// A key already being present is fine — sign with it.
			if (!isKeyExistsError(describeError(generateError))) throw generateError;
		}
		return await signModPackage(packagePath);
	}
}
