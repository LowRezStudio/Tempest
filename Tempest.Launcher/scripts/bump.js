import { readFileSync, writeFileSync } from "node:fs";
import { stdin } from "node:process";
import { createInterface } from "node:readline/promises";
import $ from "dax";

const rl = createInterface({ input: stdin });
const ask = (q, d) => rl.question(d ? `${q} [${d}]: ` : `${q}: `).then((a) => a.trim() || d || "");

async function multiline(q) {
	console.log(`\n${q} (empty line to finish):\n`);
	const lines = [];
	while (true) {
		const line = await rl.question("");
		if (!line.trim()) break;
		lines.push(line);
	}
	return lines.join("\n");
}

async function main() {
	const pkg = JSON.parse(readFileSync("Tempest.Launcher/package.json", "utf8"));
	const cur = pkg.version;

	$.log("=== Tempest Version Bump ===\n");
	$.log(`Current version: ${cur}\n`);

	const dash = cur.lastIndexOf("-");
	const suggested =
		dash > 0 && /^\d+$/.test(cur.slice(dash + 1))
			? `${cur.slice(0, dash)}-${Number(cur.slice(dash + 1)) + 1}`
			: cur;

	const ver = await ask("New version", suggested);
	if (!ver) return ($.log("Aborted."), rl.close());

	const notes = await multiline("Enter patch notes");
	if (!notes) return ($.log("Aborted."), rl.close());

	$.log(`\n--- Summary ---`);
	$.log(`Version: ${cur} → ${ver}`);
	$.log("Patch notes:");
	$.log(notes);
	$.log("---------------\n");

	if (!(await $.confirm("Proceed with bump?"))) return ($.log("Aborted."), rl.close());

	pkg.version = ver;
	writeFileSync("Tempest.Launcher/package.json", `${JSON.stringify(pkg, null, "\t")}\n`);

	const tauri = JSON.parse(readFileSync("Tempest.Launcher/src-tauri/tauri.conf.json", "utf8"));
	tauri.version = ver;
	writeFileSync(
		"Tempest.Launcher/src-tauri/tauri.conf.json",
		`${JSON.stringify(tauri, null, "\t")}\n`,
	);

	const cargoPath = "Tempest.Launcher/src-tauri/Cargo.toml";
	let cargo = readFileSync(cargoPath, "utf8");
	if (!cargo.includes(`version = "${cur}"`)) {
		$.logError("ERROR: version not found in Cargo.toml");
		process.exit(1);
	}
	writeFileSync(cargoPath, cargo.replace(`version = "${cur}"`, `version = "${ver}"`));

	const lockPath = "Tempest.Launcher/src-tauri/Cargo.lock";
	let lock = readFileSync(lockPath, "utf8");
	const marker = `name = "tempest-launcher"\nversion = "${cur}"`;
	if (!lock.includes(marker)) {
		$.logError("ERROR: version not found in Cargo.lock");
		process.exit(1);
	}
	writeFileSync(lockPath, lock.replace(marker, `name = "tempest-launcher"\nversion = "${ver}"`));

	const electronPkg = JSON.parse(readFileSync("Tempest.Launcher/electron/package.json", "utf8"));
	electronPkg.version = ver;
	writeFileSync(
		"Tempest.Launcher/electron/package.json",
		`${JSON.stringify(electronPkg, null, "\t")}\n`,
	);

	$.log("✓ Files updated");

	const tag = `v${ver}`;
	await $`git add -A`;
	await $`git commit -F -`.stdin(`chore: bump to ${tag}\n\n${notes}`);
	$.log("✓ Committed");
	await $`git tag -a ${tag} -F -`.stdin(notes);
	$.log(`✓ Tagged ${tag}`);

	if (await $.confirm("Push commit and tag to origin?")) {
		await $`git push`;
		await $`git push origin ${tag}`;
		$.log("✓ Pushed");
	} else {
		$.log("Skipped push. Run:");
		$.log(`  git push && git push origin ${tag}`);
	}

	$.log("\nDone!");
	rl.close();
}

main().catch((e) => {
	$.logError(e);
	process.exit(1);
});
