import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

interface BubblePayload {
  text: string;
  visible: boolean;
}

function initialTextFromUrl(): string {
  return new URLSearchParams(window.location.search).get("text") ?? "";
}

export function BubbleWindow() {
  const [text, setText] = useState(initialTextFromUrl);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      if (initialTextFromUrl()) {
        setVisible(true);
      }
    });

    let unlisten: (() => void) | null = null;
    void listen<BubblePayload>("bubble:update", (event) => {
      if (event.payload.text) {
        setText(event.payload.text);
      }
      setVisible(event.payload.visible);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      cancelAnimationFrame(raf);
      unlisten?.();
    };
  }, []);

  return (
    <div className="bubble-root">
      <div className={`bubble-pill ${visible ? "is-visible" : ""}`}>
        <span className="bubble-text">{text}</span>
      </div>
    </div>
  );
}
