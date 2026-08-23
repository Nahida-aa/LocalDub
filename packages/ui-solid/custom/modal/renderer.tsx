// import { createStore, useSelector, useStore } from '@tanstack/solid-store';
import { createStore } from "solid-js/store";
import { Modal, type ModalProps } from "./modal";
import type { JSX } from "solid-js";

export type ModalState = Omit<
  ModalProps,
  "children" | "Trigger" | "onOpenChange" | "defaultOpen"
> & {
  content?: ModalProps["children"];
  pending?: boolean;
};

export const [modalRendererStore, setStore] = createStore<ModalState>({
  open: false,
  pending: false,
});

const setOpen = (open: boolean) => {
  if (!open) {
    return closeModal();
  }
  setStore("open", open);
};

type OpenModalOptions = Omit<ModalState, "open" | "content">;
export const openModal = (content: ModalProps["children"], options: OpenModalOptions = {}) => {
  console.log("openModal");
  setStore((prev) => ({
    ...options,
    open: true,
    content,
  }));
};
export const closeModal = () => {
  console.log("closeModal");
  setStore((prev) => ({
    title: undefined,
    description: undefined,
    open: false,
  }));
};

// export const useModal = () => {
// 	const state = useSelector(modalRendererStore, (state) => state);
// 	return { ...state, setOpen, setPending, openModal, closeModal };
// };

export const ModalRenderer = () => {
  // const {
  // 	open,
  // 	pending,
  // 	setOpen,
  // 	content,
  // 	title,
  // 	description,
  // 	closeOnOverlayClick,
  // 	size,
  // 	showCloseButton,
  // 	...props
  // } = useModal();
  return (
    <Modal
      {...modalRendererStore}
      children={modalRendererStore.content}
      onOpenChange={setOpen}
      // open={modalRendererStore.open}
      // title={modalRendererStore.title}
      // description={modalRendererStore.description}
      // closeOnOverlayClick={modalRendererStore.closeOnOverlayClick}
      // size={modalRendererStore.size}
      // children={modalRendererStore.content}
      // showCloseButton={modalRendererStore.showCloseButton}
      // pending={modalRendererStore.pending}
    />
  );
};
