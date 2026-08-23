import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import HoverPanel from "./components/HoverPanel";
import "./index.css";

const isHoverPanel = new URLSearchParams(window.location.search).get("view") === "tray-hover";
const Root = isHoverPanel ? HoverPanel : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
