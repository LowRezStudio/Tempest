const { ipcMain, app } = require("electron");
const path = require("node:path");

const sqliteDbs = new Map();

function resolveDbPath(connString) {
	const dbPath = connString.replace(/^sqlite:/, "");
	if (path.isAbsolute(dbPath)) return dbPath;
	return path.join(app.getPath("userData"), dbPath);
}

function ensureDb(connString) {
	if (!sqliteDbs.has(connString)) {
		const { DatabaseSync } = require("node:sqlite");
		sqliteDbs.set(connString, new DatabaseSync(resolveDbPath(connString)));
	}
	return sqliteDbs.get(connString);
}

function convertQuery(sql) {
	const positions = [];
	const newSql = sql.replaceAll(/\$(\d+)/g, (_, n) => {
		positions.push(Number.parseInt(n, 10) - 1);
		return "?";
	});
	return { sql: newSql, positions };
}

function mapBindings(bindValues, positions) {
	if (!bindValues || bindValues.length === 0) return [];
	return positions.map((i) => bindValues[i]);
}

ipcMain.handle("sql:load", (_event, { path: connString }) => {
	ensureDb(connString);
});

ipcMain.handle("sql:execute", (_event, { db: connString, query, values }) => {
	const db = ensureDb(connString);
	const { sql, positions } = convertQuery(query);
	const stmt = db.prepare(sql);
	const result =
		mapBindings(values, positions).length > 0
			? stmt.run(...mapBindings(values, positions))
			: stmt.run();
	return { rowsAffected: result.changes, lastInsertId: Number(result.lastInsertRowid) };
});

ipcMain.handle("sql:select", (_event, { db: connString, query, values }) => {
	const db = ensureDb(connString);
	const { sql, positions } = convertQuery(query);
	const stmt = db.prepare(sql);
	const bindings = mapBindings(values, positions);
	return bindings.length > 0 ? stmt.all(...bindings) : stmt.all();
});

ipcMain.handle("sql:close", (_event, { db: connString }) => {
	const db = sqliteDbs.get(connString);
	if (db) {
		db.close();
		sqliteDbs.delete(connString);
	}
});
