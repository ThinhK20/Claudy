import { getCurrentWindow } from "@tauri-apps/api/window";
import MainApp from "@/windows/MainApp";
import OverlayPage from "@/windows/OverlayPage";
import AssistantPage from "@/windows/AssistantPage";

function App() {
  switch (getCurrentWindow().label) {
    case "overlay":
      return <OverlayPage />;
    case "assistant":
      return <AssistantPage />;
    default:
      return <MainApp />;
  }
}

export default App;
