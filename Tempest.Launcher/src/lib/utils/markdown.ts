import DOMPurify from "dompurify";
import { marked } from "marked";

// Mod READMEs come from untrusted authors and marked keeps raw HTML, so its
// output is sanitized before {@html} renders it.
const purifier = DOMPurify();
purifier.addHook("afterSanitizeAttributes", (node) => {
	// Links open through openUrl() on purpose; never let them target a new window.
	if (node.tagName === "A") {
		node.removeAttribute("target");
		node.setAttribute("rel", "noopener noreferrer");
	}
});

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
