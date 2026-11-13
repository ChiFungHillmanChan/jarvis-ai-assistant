import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/global.css";
import "./styles/animations.css";
import App from "./App";
import { UiLocaleProvider } from "./i18n";
import ErrorBoundary from "./components/ErrorBoundary";
import { watchUserActivity } from "./lib/user-activity";

watchUserActivity();

if (import.meta.env.DEV && new URLSearchParams(location.search).has("workspace-preview")) {
  const { installWorkspacePreview } = await import("./dev/workspacePreview");
  installWorkspacePreview();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <UiLocaleProvider>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </UiLocaleProvider>
  </React.StrictMode>,
);
