// One sign-in dialog for the whole app: Settings, an instance's settings and a launch that
// cannot sign in all open this one. The accounts themselves are a query
// (`$lib/queries/epic`); everything token-shaped lives in the host.

// launcherAppClient2: Epic's launcher client, the one allowed to mint the
// exchange codes a game client is started with.
const CLIENT_ID = "34a02cf8f4414e29b15921876da36f9a";
const REDIRECT_URL = `https://www.epicgames.com/id/api/redirect?clientId=${CLIENT_ID}&responseType=code`;

/** Sign in here, then paste the code the page shows into the dialog. */
export const EPIC_LOGIN_URL = `https://www.epicgames.com/id/login?redirectUrl=${encodeURIComponent(
	REDIRECT_URL,
)}`;

/** Open, and why: `note` replaces the dialog's usual line. */
export const epicLogin = $state<{ open: boolean; note?: string }>({ open: false });

/**
 * Opens the sign-in dialog.
 * @param note - Why the sign-in is needed, when the caller knows better than the dialog.
 */
export const openEpicLogin = (note?: string) => {
	epicLogin.note = note;
	epicLogin.open = true;
};
