import { getCurrentWindow } from "@tauri-apps/api/window";
import MainApp from "@/windows/MainApp";
import OverlayPage from "@/windows/OverlayPage";

function App() {
  return getCurrentWindow().label === "overlay" ? <OverlayPage /> : <MainApp />;
}

export default App;
