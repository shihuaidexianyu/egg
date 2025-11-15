import { useMemo } from "react";
import "./styles/theme-dark.css";
import "./styles/base.css";
import { LauncherWindow } from "./components/LauncherWindow";
import { SettingsWindow } from "./components/SettingsWindow";

const resolveWindowIntent = () => {
  const params = new URLSearchParams(window.location.search);
  return params.get("window") ?? "main";
};

function App() {
  const windowIntent = useMemo(resolveWindowIntent, []);

  if (windowIntent === "settings") {
    return <SettingsWindow />;
  }

  return <LauncherWindow />;
}

export default App;
