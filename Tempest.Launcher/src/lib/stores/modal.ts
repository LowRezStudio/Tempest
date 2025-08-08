import { writable } from 'svelte/store';

interface ModalState {
  isOpen: boolean;
  title?: string;
  description?: string;
  content?: any;
}

const initialState: ModalState = {
  isOpen: false,
  title: '',
  description: '',
  content: null
};

export const modalStore = writable<ModalState>(initialState);

export const openModal = (options: Partial<ModalState>) => {
  modalStore.update(state => ({
    ...state,
    isOpen: true,
    ...options
  }));
};

export const closeModal = () => {
  modalStore.update(state => ({
    ...state,
    isOpen: false
  }));
};