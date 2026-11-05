// Epic Games OAuth: the launcher client's authorization-code login, the accounts
// it is signed in to, and the one-time exchange codes a game is started with.
// Tokens never cross the bridge; the renderer only sees who is signed in.
//
// Endpoints and the launcherAppClient2 credentials come from
// https://github.com/LeleDerGrasshalmi/FortniteEndpointsDocumentation (Account
// Service → Authentication); exchange codes are the same hand-off the Epic
// Games launcher uses to start a game (`-AUTH_TYPE=exchangecode -AUTH_PASSWORD=<code>`).
import fs from "node:fs";
import path from "node:path";
import { app, net } from "electron";
import { handle } from "./ipc/handle.js";
import type { EpicAccount } from "./ipc/contract.js";

const OAUTH_BASE = "https://account-public-service-prod.ol.epicgames.com";
const TOKEN_URL = `${OAUTH_BASE}/account/api/oauth/token`;
const EXCHANGE_URL = `${OAUTH_BASE}/account/api/oauth/exchange`;

// `launcherAppClient2`: the client Epic's own launcher authenticates with. It is
// allowed to mint exchange codes, which game clients accept.
const CLIENT_ID = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET = "daafbccc737745039dffe53d94fc76cf";

// Paladins' Epic Games Store artifact. The launch line below mirrors what the
// Epic launcher hands a game (the same set legendary builds in `get_launch_parameters`).
const PALADINS_APP_NAME = "Antbird";
const PALADINS_SANDBOX_ID = "badb0ee71b474ed591ec43212547cfc8";

// The token endpoint serves launcher clients; anything else is throttled harder.
const USER_AGENT =
	"UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";

const BASIC = `basic ${Buffer.from(`${CLIENT_ID}:${CLIENT_SECRET}`).toString("base64")}`;

type StoredAccount = {
	accountId: string;
	displayName: string;
	refreshToken: string;
	/** Unix ms when the refresh token stops working; null when Epic did not say. */
	refreshExpiresAt: number | null;
	addedAt: number;
	lastUsedAt: number | null;
};

type AccountFile = {
	/** The account instances use when they do not name one. */
	selectedId: string | null;
	accounts: StoredAccount[];
};

type TokenResponse = {
	access_token?: string;
	expires_in?: number;
	expires_at?: string;
	refresh_token?: string;
	refresh_expires?: number;
	refresh_expires_at?: string;
	account_id?: string;
	in_app_id?: string;
	displayName?: string;
	errorCode?: string;
	errorMessage?: string;
};

type CachedToken = { token: string; expiresAt: number };

// Epic answers quickly or not at all: a hung request must not hang a launch forever.
const REQUEST_TIMEOUT_MS = 15_000;

const accountsPath = () => path.join(app.getPath("userData"), "epic", "accounts.json");

const isAccount = (value: unknown): value is StoredAccount => {
	if (!value || typeof value !== "object") return false;
	const account = value as Partial<StoredAccount>;
	return (
		typeof account.accountId === "string" &&
		!!account.accountId &&
		typeof account.displayName === "string" &&
		typeof account.refreshToken === "string" &&
		!!account.refreshToken
	);
};

function readAccounts(): AccountFile {
	try {
		const parsed = JSON.parse(fs.readFileSync(accountsPath(), "utf8")) as Partial<AccountFile>;
		const accounts = Array.isArray(parsed.accounts) ? parsed.accounts.filter(isAccount) : [];
		// A removed or renamed account must not leave a dangling selection behind.
		const selectedId =
			accounts.find((account) => account.accountId === parsed.selectedId)?.accountId ??
			accounts[0]?.accountId ??
			null;
		return { selectedId, accounts };
	} catch {
		return { selectedId: null, accounts: [] };
	}
}

function writeAccounts(file: AccountFile): void {
	const target = accountsPath();
	fs.mkdirSync(path.dirname(target), { recursive: true, mode: 0o700 });
	// Write-then-rename so a crash never leaves a half-written account list.
	const temporary = `${target}.tmp`;
	fs.writeFileSync(temporary, JSON.stringify(file, null, "\t"), { mode: 0o600 });
	fs.renameSync(temporary, target);
}

// One change at a time: a launch refreshing its token and a sign-in in Settings must
// not drop each other's write, nor race on the same temporary file.
let writes: Promise<unknown> = Promise.resolve();

/**
 * Reads, changes and writes the account file after the changes queued before it.
 * @param change - Called with the freshly read file; must be synchronous.
 * @returns What `change` returned.
 */
function mutateAccounts<T>(change: (file: AccountFile) => T): Promise<T> {
	const next = writes.then(() => {
		const file = readAccounts();
		const result = change(file);
		writeAccounts(file);
		return result;
	});
	writes = next.catch(() => {});
	return next;
}

const summarize = (account: StoredAccount): EpicAccount => ({
	accountId: account.accountId,
	displayName: account.displayName,
	addedAt: account.addedAt,
	lastUsedAt: account.lastUsedAt,
	refreshExpiresAt: account.refreshExpiresAt,
});

const listAccounts = () => {
	const file = readAccounts();
	return { accounts: file.accounts.map(summarize), selectedId: file.selectedId };
};

/**
 * Accepts a bare authorization code as well as the whole `redirectUrl` the Epic
 * page hands out (`https://accounts.epicgames.com/fnauth?code=...`).
 * @param input - What the user pasted.
 * @returns The code, or null when nothing usable is in there.
 */
function parseAuthorizationCode(input: string): string | null {
	const text = input.trim();
	if (!text) return null;

	try {
		const url = new URL(text);
		const code = url.searchParams.get("code");
		if (code) return code.trim();
	} catch {
		/* not a URL: treat it as the code itself */
	}

	// A JSON body copied from the redirect endpoint.
	try {
		const parsed = JSON.parse(text) as { authorizationCode?: unknown };
		if (typeof parsed.authorizationCode === "string") return parsed.authorizationCode.trim();
	} catch {
		/* plain code */
	}

	return /^[0-9a-zA-Z]+$/.test(text) ? text : null;
}

// A launch without a usable sign-in is not a failure: the renderer asks for one instead.
class SignInRequired extends Error {}

const describeFailure = (payload: TokenResponse, status: number): Error => {
	switch (payload.errorCode) {
		case "errors.com.epicgames.account.oauth.authorization_code_not_found":
		case "errors.com.epicgames.account.oauth.authorization_code_expired": {
			return new Error(
				"That code is not valid or has expired. Open the sign-in page again for a new one.",
			);
		}
		case "errors.com.epicgames.account.oauth.invalid_grant":
		case "errors.com.epicgames.account.oauth.invalid_refresh_token": {
			return new SignInRequired("The Epic Games session has expired. Sign in again.");
		}
		case "errors.com.epicgames.common.oauth.invalid_client": {
			return new Error(
				"Epic Games rejected the launcher client. The launcher may need an update.",
			);
		}
		default: {
			return new Error(
				payload.errorMessage ?? `Epic Games rejected the sign-in (HTTP ${status}).`,
			);
		}
	}
};

async function postToken(form: Record<string, string>): Promise<TokenResponse> {
	const response = await net
		.fetch(TOKEN_URL, {
			method: "POST",
			headers: {
				accept: "application/json",
				"content-type": "application/x-www-form-urlencoded",
				"user-agent": USER_AGENT,
				authorization: BASIC,
			},
			body: new URLSearchParams(form).toString(),
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
		})
		.catch((error: unknown) => {
			throw new Error(
				`Could not reach Epic Games: ${error instanceof Error ? error.message : String(error)}`,
			);
		});
	const payload = (await response.json().catch(() => ({}))) as TokenResponse;
	if (!response.ok || payload.errorCode) {
		throw describeFailure(payload, response.status);
	}
	return payload;
}

const tokens = new Map<string, CachedToken>();
const refreshing = new Map<string, Promise<CachedToken>>();

function remember(accountId: string, response: TokenResponse): CachedToken {
	// Renew a minute early so a launch never races the expiry.
	const expiresAt = Date.now() + Math.max(0, (response.expires_in ?? 7200) - 60) * 1000;
	const cached = { token: response.access_token ?? "", expiresAt };
	tokens.set(accountId, cached);
	return cached;
}

const parseExpiry = (value: string | undefined): number | null => {
	if (!value) return null;
	const parsed = Date.parse(value);
	return Number.isNaN(parsed) ? null : parsed;
};

function refreshExpiryOf(response: TokenResponse, fallback: number | null): number | null {
	const absolute = parseExpiry(response.refresh_expires_at);
	if (absolute !== null) return absolute;
	if (typeof response.refresh_expires === "number") {
		return Date.now() + response.refresh_expires * 1000;
	}
	return fallback;
}

const expiredMessage = (account: StoredAccount) =>
	`The Epic Games session for ${account.displayName} has expired. Sign in again in Settings → Epic Games.`;

async function refreshSession(account: StoredAccount): Promise<CachedToken> {
	if (account.refreshExpiresAt !== null && account.refreshExpiresAt <= Date.now()) {
		throw new SignInRequired(expiredMessage(account));
	}

	const response = await postToken({
		grant_type: "refresh_token",
		refresh_token: account.refreshToken,
		token_type: "eg1",
	});
	if (!response.access_token) throw new SignInRequired(expiredMessage(account));

	// Keep the stored copy in step: Epic rotates refresh tokens.
	await mutateAccounts((file) => {
		const stored = file.accounts.find((entry) => entry.accountId === account.accountId);
		if (!stored) return;
		stored.refreshToken = response.refresh_token ?? stored.refreshToken;
		stored.refreshExpiresAt = refreshExpiryOf(response, stored.refreshExpiresAt);
		if (response.displayName) stored.displayName = response.displayName;
	});

	return remember(account.accountId, response);
}

async function accessTokenFor(account: StoredAccount): Promise<string> {
	const cached = tokens.get(account.accountId);
	if (cached && cached.expiresAt > Date.now()) return cached.token;

	// One refresh at a time: two launches must not rotate the same token twice.
	const pending = refreshing.get(account.accountId);
	if (pending) return (await pending).token;

	const refresh = refreshSession(account).finally(() => refreshing.delete(account.accountId));
	refreshing.set(account.accountId, refresh);
	return (await refresh).token;
}

/**
 * Ends the account's Epic session server-side, so the tokens it kept cannot be reused.
 * Best effort: an expired session (or an account that is already gone) is fine too.
 * @param account - The account whose tokens are about to be forgotten.
 */
async function killSession(account: StoredAccount): Promise<void> {
	const token = await accessTokenFor(account);
	const response = await net.fetch(
		`${OAUTH_BASE}/account/api/oauth/sessions/kill/${encodeURIComponent(token)}`,
		{
			method: "DELETE",
			headers: {
				accept: "application/json",
				"user-agent": USER_AGENT,
				authorization: `bearer ${token}`,
			},
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
		},
	);
	// 401/404: the session is already over; anything else that fails is worth logging.
	if (!response.ok && response.status !== 401 && response.status !== 404) {
		throw new Error(`Epic Games did not end the session (HTTP ${response.status}).`);
	}
}

async function exchangeCodeFor(account: StoredAccount): Promise<string> {
	const token = await accessTokenFor(account);
	const response = await net
		.fetch(EXCHANGE_URL, {
			headers: {
				accept: "application/json",
				"user-agent": USER_AGENT,
				authorization: `bearer ${token}`,
			},
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
		})
		.catch((error: unknown) => {
			throw new Error(
				`Could not reach Epic Games: ${error instanceof Error ? error.message : String(error)}`,
			);
		});
	const payload = (await response.json().catch(() => ({}))) as {
		code?: string;
		errorCode?: string;
		errorMessage?: string;
	};
	if (!response.ok || !payload.code) {
		throw new Error(
			payload.errorMessage ??
				`Epic Games did not return a sign-in (HTTP ${response.status}).`,
		);
	}

	await mutateAccounts((file) => {
		const stored = file.accounts.find((entry) => entry.accountId === account.accountId);
		if (stored) stored.lastUsedAt = Date.now();
	});

	return payload.code;
}

handle("epic:accounts", () => listAccounts());

handle("epic:login", async (_event, { code }) => {
	const authorizationCode = parseAuthorizationCode(code);
	if (!authorizationCode) {
		throw new Error("Paste the code from the Epic Games sign-in page.");
	}

	const response = await postToken({
		grant_type: "authorization_code",
		code: authorizationCode,
		token_type: "eg1",
	});
	const accountId = response.account_id ?? response.in_app_id;
	if (!response.access_token || !accountId) {
		throw new Error("Epic Games did not return an account.");
	}

	const stored = await mutateAccounts((file) => {
		const existing = file.accounts.find((entry) => entry.accountId === accountId);
		const account: StoredAccount = {
			accountId,
			displayName: response.displayName ?? existing?.displayName ?? accountId,
			refreshToken: response.refresh_token ?? existing?.refreshToken ?? "",
			refreshExpiresAt: refreshExpiryOf(response, existing?.refreshExpiresAt ?? null),
			addedAt: existing?.addedAt ?? Date.now(),
			lastUsedAt: existing?.lastUsedAt ?? null,
		};
		if (!account.refreshToken) {
			throw new Error("Epic Games did not return a refresh token.");
		}
		file.accounts = [
			...file.accounts.filter((entry) => entry.accountId !== accountId),
			account,
		];
		file.selectedId = accountId;
		return account;
	});
	remember(accountId, response);

	return summarize(stored);
});

handle("epic:remove", async (_event, { accountId }) => {
	const account = readAccounts().accounts.find((entry) => entry.accountId === accountId);
	if (account) {
		// The account is forgotten either way; an offline or already-dead session cannot block it.
		await killSession(account).catch((error: unknown) => {
			console.warn("[epic] could not end the session:", error);
		});
	}
	tokens.delete(accountId);
	return mutateAccounts((file) => {
		file.accounts = file.accounts.filter((entry) => entry.accountId !== accountId);
		if (file.selectedId === accountId) file.selectedId = file.accounts[0]?.accountId ?? null;
		return { accounts: file.accounts.map(summarize), selectedId: file.selectedId };
	});
});

handle("epic:select", (_event, { accountId }) =>
	mutateAccounts((file) => {
		if (!file.accounts.some((entry) => entry.accountId === accountId)) {
			throw new Error("That Epic Games account is not signed in anymore.");
		}
		file.selectedId = accountId;
	}),
);

handle("epic:launch-args", async (_event, { accountId }) => {
	try {
		const file = readAccounts();
		const id = accountId ?? file.selectedId;
		const account = file.accounts.find((entry) => entry.accountId === id);
		if (!account) {
			throw new SignInRequired(
				accountId
					? "The Epic Games account this instance uses is not signed in anymore."
					: "No Epic Games account is signed in.",
			);
		}

		const code = await exchangeCodeFor(account);
		// `-epic` is Paladins' own switch: without it the client skips its Epic
		// subsystem entirely (`Epic Online Services API Disabled!`).
		return [
			"-epic",
			"-AUTH_LOGIN=unused",
			`-AUTH_PASSWORD=${code}`,
			"-AUTH_TYPE=exchangecode",
			`-epicapp=${PALADINS_APP_NAME}`,
			"-epicenv=Prod",
			"-EpicPortal",
			`-epicusername=${account.displayName}`,
			`-epicuserid=${account.accountId}`,
			`-epiclocale=${app.getLocale().split("-")[0] || "en"}`,
			`-epicsandboxid=${PALADINS_SANDBOX_ID}`,
		];
	} catch (error) {
		// No usable account: the renderer asks for a sign-in instead of failing the launch.
		if (error instanceof SignInRequired) return null;
		throw error;
	}
});
