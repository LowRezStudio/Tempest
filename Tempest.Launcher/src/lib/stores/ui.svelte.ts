export const instanceWizardOpen = $state({ value: false });
export const hostServerWizardOpen = $state({ value: false });
export const joinServerWizardOpen = $state({ value: false });
export const appCloseLobbyWizardOpen = $state({ value: false });

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

export const toasts = $state({ value: [] as Toast[] });

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

	toasts.value = [...toasts.value, toast];

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

	toasts.value = toasts.value.filter((toast) => toast.id !== id);
};

export const clearToasts = () => {
	for (const timeout of toastTimeouts.values()) {
		clearTimeout(timeout);
	}
	toastTimeouts.clear();
	toasts.value = [];
};
