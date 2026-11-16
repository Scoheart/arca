import { createRoot } from "react-dom/client";
import { App } from "./App";
import { sessionToken } from "./api";
import "./styles.css";

const root = document.getElementById("root");
if (root) createRoot(root).render(<App token={sessionToken()} />);
