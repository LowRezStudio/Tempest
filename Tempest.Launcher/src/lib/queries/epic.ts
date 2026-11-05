import { createMutation, createQuery, useQueryClient } from "@tanstack/svelte-query";
import { invoke } from "$lib/platform/api";
import type { EpicAccount, EpicAccountList } from "$ipc/contract";

const ACCOUNTS_KEY = ["epic", "accounts"];

/**
 * Who is signed in to Epic Games, and which account instances use by default.
 * @returns The query.
 */
export const createEpicAccountsQuery = () =>
	createQuery(() => ({
		queryKey: ACCOUNTS_KEY,
		queryFn: (): Promise<EpicAccountList> => invoke("epic:accounts"),
	}));

// The host answers each change with the new list; refetching keeps one rule for all three
// and costs a local IPC call.
const createAccountMutation = <V, D>(run: (variables: V) => Promise<D>) => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: run,
		onSuccess: () => void queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY }),
	}));
};

/**
 * Exchanges a pasted authorization code from Epic's sign-in page for an account.
 * @returns The mutation.
 */
export const createEpicLoginMutation = () =>
	createAccountMutation((code: string): Promise<EpicAccount> => invoke("epic:login", { code }));

/**
 * Picks the account instances use when they do not name one.
 * @returns The mutation.
 */
export const createEpicSelectMutation = () =>
	createAccountMutation((accountId: string) => invoke("epic:select", { accountId }));

/**
 * Signs the account out and forgets it.
 * @returns The mutation.
 */
export const createEpicRemoveMutation = () =>
	createAccountMutation((accountId: string) => invoke("epic:remove", { accountId }));
