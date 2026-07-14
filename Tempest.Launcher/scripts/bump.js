import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { stdin, stdout } from "node:process";
import { createInterface } from "node:readline/promises";

const rl = createInterface({ input: stdin, output: stdout });
const ask = (q, d) => rl.question(d ? `${q} [${d}]: ` : `${q}: `).then((a) => a.trim() || d || "");
const confirm = (q) => rl.question(`${q} [y/N]: `).then((a) => a.trim().toLowerCase() === "y");

async function main() {
	const pkg = JSON.parse(readFileSync("package.json", "utf8"));
	const cur = pkg.version;

	console.log("=== Tempest Version Bump ===\n");
	console.log(`Current version: ${cur}`);

	const dash = cur.lastIndexOf("-");
	const suggested =
		dash > 0 && /^\d+$/.test(cur.slice(dash + 1))
			? `${cur.slice(0, dash)}-${Number(cur.slice(dash + 1)) + 1}`
			: cur;

	console.log(`Suggested: ${suggested}\n`);

	const ver = await ask("New version", suggested);
	if (!ver) return (console.log("Aborted."), rl.close());

	const notes = await ask("Patch notes");
	if (!notes) return (console.log("Aborted."), rl.close());

	console.log("\n--- Summary ---");
	console.log(`Version: ${cur} -> ${ver}`);
	console.log(`Notes: ${notes}\n`);

	if (!(await confirm("Proceed with bump?"))) return (console.log("Aborted."), rl.close());

	pkg.version = ver;
	writeFileSync("package.json", `${JSON.stringify(pkg, null, "\t")}\n`);
	console.log("  updated package.json");

	const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
	tauri.version = ver;
	writeFileSync("src-tauri/tauri.conf.json", `${JSON.stringify(tauri, null, "\t")}\n`);
	console.log("  updated src-tauri/tauri.conf.json");

	const cargoPath = "src-tauri/Cargo.toml";
	let cargo = readFileSync(cargoPath, "utf8");
	if (!cargo.includes(`version = "${cur}"`)) {
		console.error(`ERROR: version not found in ${cargoPath}`);
		process.exit(1);
	}
	writeFileSync(cargoPath, cargo.replace(`version = "${cur}"`, `version = "${ver}"`));
	console.log("  updated src-tauri/Cargo.toml");

	const lockPath = "src-tauri/Cargo.lock";
	try {
		let lock = readFileSync(lockPath, "utf8");
		const markers = [`name = "tempest-launcher"\nversion = "${cur}"`];
		for (const m of markers) {
			if (lock.includes(m)) lock = lock.replace(m, m.replace(`"${cur}"`, `"${ver}"`));
		}
		writeFileSync(lockPath, lock);
		console.log("  updated src-tauri/Cargo.lock");
	} catch {}

	const electronPkg = JSON.parse(readFileSync("electron/package.json", "utf8"));
	electronPkg.version = ver;
	writeFileSync("electron/package.json", `${JSON.stringify(electronPkg, null, "\t")}\n`);
	console.log("  updated electron/package.json");

	console.log("\nAll files updated.\n");

	const tag = `v${ver}`;
	const msg = `chore: bump to ${tag}\n\n${notes}`;
	execSync("git add -A", { stdio: "inherit" });
	execSync("git commit -F -", { input: msg, stdio: ["pipe", "inherit", "inherit"] });
	console.log("  committed");
	execSync(`git tag -a ${tag} -F -`, { input: notes, stdio: ["pipe", "inherit", "inherit"] });
	console.log(`  tagged ${tag}`);

	if (await confirm("Push commit and tag to origin?")) {
		execSync("git push", { stdio: "inherit" });
		execSync(`git push origin ${tag}`, { stdio: "inherit" });
		console.log("  pushed");
	} else {
		console.log("\nSkipped push. To push later:");
		console.log(`  git push && git push origin ${tag}`);
	}

	console.log("\nDone!");
	rl.close();
}

main().catch((e) => {
	console.error(e);
	process.exit(1);
});
