import { BubbleWindow } from "./components/bubble/BubbleWindow";
import { GreetingPanel } from "./components/panel/GreetingPanel";
import { PetWindow } from "./components/pet/PetWindow";

type WindowRole = "pet" | "bubble" | "panel";

function getWindowRole(): WindowRole {
  const param = new URLSearchParams(window.location.search).get("window");
  if (param === "bubble" || param === "panel") {
    return param;
  }
  return "pet";
}

export function App() {
  const role = getWindowRole();
  if (role === "bubble") return <BubbleWindow />;
  if (role === "panel") return <GreetingPanel />;
  return <PetWindow />;
}
