import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/sora/latin-300.css";
import "./styles.css";
import App from "./App";
import { upgradeAddress } from "./settings";

// An old address (#household/...) becomes the new one (#settings/...) before
// anything reads it: now, and whenever a link leads to one. This listener
// comes first, so every screen that follows the address sees the new one.
upgradeAddress();
window.addEventListener("hashchange", upgradeAddress);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
