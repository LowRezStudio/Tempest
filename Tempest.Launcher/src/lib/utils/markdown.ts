import DOMPurify from "dompurify";
import { marked } from "marked";

// Mod READMEs come from untrusted authors (a local .tempest file, a third-party
// repository or the official catalogue), and marked keeps raw HTML. Rendering its
// output through {@html} without sanitizing let a README run script in the
// renderer, which reaches every IPC channel of the preload bridge.
const purifier = DOMPurify();
purifier.addHook("afterSanitizeAttributes", (node) => {
	// Links open through openUrl() on purpose; never let them target a new window.
	if (node.tagName === "A") {
		node.removeAttribute("target");
		node.setAttribute("rel", "noopener noreferrer");
	}
});

/**
 * Renders untrusted Markdown to HTML that is safe for {@html}.
 * @param source - Markdown written by a mod author.
 * @returns Sanitized HTML: no scripts, event handlers, frames, forms or javascript: URLs (DOMPurify's default URI policy).
 */
export function renderUntrustedMarkdown(source: string): string {
	const html = marked.parse(source, { async: false }) as string;
	return purifier.sanitize(html, {
		USE_PROFILES: { html: true },
		FORBID_TAGS: [
			"style",
			"iframe",
			"frame",
			"object",
			"embed",
			"form",
			"input",
			"button",
			"base",
			"meta",
			"link",
		],
		FORBID_ATTR: ["style", "srcset", "formaction", "action"],
	});
}
