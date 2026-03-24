import React from "react";
import ReactDOM from "react-dom/client";
import Wallpaper from "./Wallpaper";

ReactDOM.createRoot(
  document.getElementById("wallpaper-root") as HTMLElement,
).render(
  <React.StrictMode>
    <Wallpaper />
  </React.StrictMode>,
);
