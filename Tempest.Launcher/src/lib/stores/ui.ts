import { atom } from "nanostores";

export const instanceWizardOpen = atom<boolean>(false);

export type ToastTone = "info" | "success" | "warning" | "error" | "neutral";

export type Toast = {
	id: string;
	title?: string;
	message: string;
	tone?: ToastTone;
	duration?: number;
	dismissible?: boolean;
};

export type ToastInput = Omit<Toast, "id">;

const defaultToastDuration = 4500;
const toastTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

const createToastId = () =>
	globalThis.crypto?.randomUUID?.() ??
	`toast-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

export const toasts = atom<Toast[]>([]);

export const addToast = (input: ToastInput) => {
	const id = createToastId();
	const toast: Toast = {
		id,
		title: input.title,
		message: input.message,
		tone: input.tone ?? "info",
		duration: input.duration ?? defaultToastDuration,
		dismissible: input.dismissible ?? true,
	};

	toasts.set([...toasts.get(), toast]);

	if (toast.duration && toast.duration > 0) {
		const timeout = setTimeout(() => {
			removeToast(id);
		}, toast.duration);
		toastTimeouts.set(id, timeout);
	}

	return id;
};

export const removeToast = (id: string) => {
	const timeout = toastTimeouts.get(id);
	if (timeout) {
		clearTimeout(timeout);
		toastTimeouts.delete(id);
	}

	toasts.set(toasts.get().filter((toast) => toast.id !== id));
};

export const clearToasts = () => {
	for (const timeout of toastTimeouts.values()) {
		clearTimeout(timeout);
	}
	toastTimeouts.clear();
	toasts.set([]);
};
