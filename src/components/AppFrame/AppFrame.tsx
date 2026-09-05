import type { JSX } from "solid-js";
import { osWindowStore } from "../../stores/global/os-window";

interface Props {
  children: JSX.Element;
}

// The window's own surface. Its rounded corners, and on GNOME the inset that
// leaves room for the client-side-decoration shadow, are the same decision:
// a floating window has both, a maximized one meets the screen edge and has
// neither. One class, one signal, and the platform layer decides in CSS.
export default function AppFrame(props: Props) {
  return (
    <div class="app-container" classList={{ "is-maximized": osWindowStore.maximized() }}>
      {props.children}
    </div>
  );
}
