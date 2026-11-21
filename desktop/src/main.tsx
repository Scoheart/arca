import "./index.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ToastProvider } from "./components/Toast";
import { applyTheme, getInitialTheme } from "./lib/theme";
import { reportFrontendLog } from "./lib/api";
import "./i18n";
import { LocaleProvider } from "./i18n/LocaleProvider";

// Apply the saved/system theme before first paint to avoid a flash.
applyTheme(getInitialTheme());

window.addEventListener("error", (event) => {
  void reportFrontendLog({
    level: "error",
    stage: "frontend.window_error",
    code: "uncaught_error",
    message: event.message || "Uncaught window error",
    details: {
      filename: event.filename || undefined,
      lineno: event.lineno || undefined,
      colno: event.colno || undefined,
    },
  });
});

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message = reason instanceof Error
    ? `${reason.name}: ${reason.message}`
    : typeof reason === "string"
      ? reason
      : "Unhandled promise rejection";
  void reportFrontendLog({
    level: "error",
    stage: "frontend.unhandled_rejection",
    code: "unhandled_rejection",
    message,
  });
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LocaleProvider>
      <ToastProvider>
        <App />
      </ToastProvider>
    </LocaleProvider>
  </React.StrictMode>,
);
