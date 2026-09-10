import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/global.css";
import "./styles/animations.css";
import App from "./App";
import ErrorBoundary from "./components/ErrorBoundary";

if (import.meta.env.DEV && new URLSearchParams(location.search).has("workspace-preview")) {
  const { installWorkspacePreview } = await import("./dev/workspacePreview");
  installWorkspacePreview();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
