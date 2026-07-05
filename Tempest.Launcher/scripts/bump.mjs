import { execSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stdin as input, stdout as output } from "node:process";
import { createInterface } from "node:readline/promises";

const rl = createInterface({ input, output });

function readJSON(path) {
	return JSON.parse(readFileSync(path, "utf8"));
}

function writeJSON(path, data) {
	writeFileSync(path, `${JSON.stringify(data, null, "\t")}\n`);
}

function getCurrentVersion() {
	return readJSON("Tempest.Launcher/package.json").version;
}

function suggestNextVersion(current) {
	const parts = current.split("-");
	if (parts.length === 2 && /^\d+$/.test(parts[1])) {
		return `${parts[0]}-${Number(parts[1]) + 1}`;
	}
	return current;
}

function updateCargoLock(oldVersion, newVersion) {
	const path = "Tempest.Launcher/src-tauri/Cargo.lock";
	let content = readFileSync(path, "utf8");
	const marker = `name = "tempest-launcher"\nversion = "${oldVersion}"`;
	if (!content.includes(marker)) {
		console.error("ERROR: Could not find tempest-launcher version in Cargo.lock");
		process.exit(1);
	}
	content = content.replace(marker, `name = "tempest-launcher"\nversion = "${newVersion}"`);
	writeFileSync(path, content);
}

async function ask(question, defaultValue) {
	const answer = await rl.question(
		defaultValue ? `${question} [${defaultValue}]: ` : `${question}: `,
	);
	return (answer.trim() !== "" ? answer.trim() : defaultValue) ?? "";
}

async function askMultiline(question) {
	console.log(`\n${question} (empty line to finish):\n`);
	const lines = [];
	while (true) {
		const line = await rl.question("");
		if (!line.trim()) break;
		lines.push(line);
	}
	return lines.join("\n");
}

async function confirm(question) {
	const answer = await rl.question(`${question} [Y/n]: `);
	return answer.trim().toLowerCase() !== "n";
}

async function main() {
	console.log("=== Tempest Version Bump ===\n");

	const currentVersion = getCurrentVersion();
	const suggested = suggestNextVersion(currentVersion);

	console.log(`Current version: ${currentVersion}\n`);

	const newVersion = await ask("New version", suggested);
	if (!newVersion) {
		console.log("Aborted.");
		process.exit(0);
	}

	const notes = await askMultiline("Enter patch notes");
	if (!notes) {
		console.log("Aborted.");
		process.exit(0);
	}

	console.log("\n--- Summary ---");
	console.log(`Version: ${currentVersion} \u2192 ${newVersion}`);
	console.log("Patch notes:");
	console.log(notes);
	console.log("---------------\n");

	if (!(await confirm("Proceed with bump?"))) {
		console.log("Aborted.");
		process.exit(0);
	}

	// Update package.json
	const pkg = readJSON("Tempest.Launcher/package.json");
	pkg.version = newVersion;
	writeJSON("Tempest.Launcher/package.json", pkg);
	console.log("\u2713 Updated package.json");

	// Update tauri.conf.json
	const tauriConf = readJSON("Tempest.Launcher/src-tauri/tauri.conf.json");
	tauriConf.version = newVersion;
	writeJSON("Tempest.Launcher/src-tauri/tauri.conf.json", tauriConf);
	console.log("\u2713 Updated tauri.conf.json");

	// Update Cargo.toml
	let cargoToml = readFileSync("Tempest.Launcher/src-tauri/Cargo.toml", "utf8");
	cargoToml = cargoToml.replace(`version = "${currentVersion}"`, `version = "${newVersion}"`);
	if (cargoToml.includes(currentVersion)) {
		console.error("ERROR: Version replacement in Cargo.toml may be incomplete");
		process.exit(1);
	}
	writeFileSync("Tempest.Launcher/src-tauri/Cargo.toml", cargoToml);
	console.log("\u2713 Updated Cargo.toml");

	// Update Cargo.lock
	updateCargoLock(currentVersion, newVersion);
	console.log("\u2713 Updated Cargo.lock");

	// Write commit message and tag annotation to temp files to avoid shell escaping issues
	const tmpDir = mkdtempSync(join(tmpdir(), "tempest-bump-"));
	const commitMsgPath = join(tmpDir, "commit-msg.txt");
	const tagMsgPath = join(tmpDir, "tag-msg.txt");

	const tag = `v${newVersion}`;
	writeFileSync(commitMsgPath, `chore: bump to ${tag}\n\n${notes}`, "utf8");
	writeFileSync(tagMsgPath, notes, "utf8");

	execSync("git add -A", { stdio: "inherit" });
	execSync(`git commit -F "${commitMsgPath}"`, { stdio: "inherit" });
	console.log("\u2713 Committed");

	execSync(`git tag -a "${tag}" -F "${tagMsgPath}"`, { stdio: "inherit" });
	console.log(`\u2713 Tagged ${tag}`);

	// Cleanup temp files
	execSync(`rmdir /s /q "${tmpDir}"`, { stdio: "ignore" });

	// Push
	if (await confirm("Push commit and tag to origin?")) {
		execSync("git push", { stdio: "inherit" });
		execSync(`git push origin ${tag}`, { stdio: "inherit" });
		console.log("\u2713 Pushed");
	} else {
		console.log("Skipped push. You can push manually:");
		console.log(`  git push && git push origin ${tag}`);
	}

	console.log("\nDone!");
	rl.close();
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
