import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { appTheme } from "./theme";

// Set data-theme before the first paint so the window never flashes the other mode.
appTheme();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
