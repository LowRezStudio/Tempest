// prettier.config.js, .prettierrc.js, prettier.config.mjs, or .prettierrc.mjs

/**
 * @see https://prettier.io/docs/configuration
 * @type {import("prettier").Config}
 */
const prettierConfig = {
	plugins: ["prettier-plugin-svelte", "prettier-plugin-tailwindcss"],
	useTabs: true,
	tabWidth: 4,
	printWidth: 100,
	semi: true,
	singleQuote: false,
	experimentalTernaries: true,
	overrides: [
		{
			files: "*.{yml,yaml}",
			options: {
				useTabs: false,
				tabWidth: 2,
				singleQuote: true,
			},
		},
		{
			files: "*.svelte",
			options: {
				parser: "svelte",
			},
		},
	],
};

/** @type {import("prettier").Config} */
const config = {
	...prettierConfig,
};

export default config;
