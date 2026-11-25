import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AnimationController } from "../../animation/controller";
import { SpriteStore } from "../../animation/spriteStore";
import type { AnimationStateName } from "../../animation/types";
import type { AppSettings } from "../../types/tracking";
import { PetCanvas } from "./PetCanvas";

const DEFAULT_PET_LOGICAL_SIZE = 128;
const DRAG_THRESHOLD_PX = 6;
const CLICK_PANEL_DELAY_MS = 160;

/** Core interactive & lifecycle states preloaded immediately; ambient expressions load on demand. */
const CORE_PET_STATES: readonly AnimationStateName[] = [
  "idle",
  "blink",
  "wave",
  "happy",
  "sleep",
  "click",
  "drag",
  "yawn",
  "late_night",
];

interface PetRuntimeSnapshot {
  tracking_paused: boolean;
  pet_visible: boolean;
  x: number;
  y: number;
}

interface PetSettingsPayload {
  petSize: number;
  cuteMomentsEnabled: boolean;
}

interface StateHintPayload {
  kind: "active" | "paused" | "idle_sleep" | "late_night";
}

interface VisibilityPayload {
  visible: boolean;
}

interface PointerSession {
  pointerId: number;
  startX: number;
  startY: number;
  dragged: boolean;
}

declare global {
  interface Window {
    __pikuDebug?: {
      play: (state: AnimationStateName) => void;
      toggleOverlay: () => void;
      triggerBubble: (text?: string) => void;
      getSnapshot: () => unknown;
    };
  }
}

export function PetWindow() {
  const [engine, setEngine] = useState<{
    store: SpriteStore;
    controller: AnimationController;
  } | null>(null);
  const [petSize, setPetSize] = useState<number>(DEFAULT_PET_LOGICAL_SIZE);
  const [showOverlay, setShowOverlay] = useState(false);

  const pointerRef = useRef<PointerSession | null>(null);
  const isDraggingRef = useRef(false);
  const dragRafRef = useRef<number | null>(null);
  const isVisibleRef = useRef(true);
  const cuteMomentsRef = useRef(true);

  useEffect(() => {
    let mounted = true;
    let activeStore: SpriteStore | null = null;
    let activeController: AnimationController | null = null;
    let unlistenHint: (() => void) | null = null;
    let unlistenVis: (() => void) | null = null;
    let unlistenSettings: (() => void) | null = null;

    const init = async () => {
      const [store, runtime, settings] = await Promise.all([
        SpriteStore.load({ states: CORE_PET_STATES }),
        invoke<PetRuntimeSnapshot>("get_pet_runtime").catch(() => ({
          tracking_paused: false,
          pet_visible: true,
          x: 0,
          y: 0,
        })),
        invoke<AppSettings>("get_settings").catch(() => null),
      ]);

      if (!mounted) {
        store.dispose();
        return;
      }

      isVisibleRef.current = runtime.pet_visible;
      const initialSize = settings?.petSize
        ? Math.min(160, Math.max(100, settings.petSize))
        : DEFAULT_PET_LOGICAL_SIZE;
      const initialCute = settings?.cuteMomentsEnabled ?? true;
      cuteMomentsRef.current = initialCute;
      setPetSize(initialSize);

      const controller = new AnimationController(store, {
        enableRandomBlink: true,
        enableAmbientMoments: initialCute,
        initialBaseState: runtime.tracking_paused ? "sleep" : "idle",
      });

      controller.onCuteMoment((moment) => {
        if (!isDraggingRef.current && isVisibleRef.current && cuteMomentsRef.current) {
          void invoke("show_bubble", { text: moment.message, durationMs: 4500 });
        }
      });

      activeStore = store;
      activeController = controller;
      setEngine({ store, controller });

      if (runtime.pet_visible && !document.hidden) {
        controller.start();
      }

      unlistenHint = await listen<StateHintPayload>("pet:state-hint", (event) => {
        const kind = event.payload.kind;
        if (kind === "late_night") {
          controller.setBaseState("late_night");
        } else if (kind === "idle_sleep") {
          controller.setBaseState("sleep", { transition: "yawn" });
        } else if (kind === "paused") {
          controller.setBaseState("sleep");
        } else {
          controller.setBaseState("idle");
        }
      });

      unlistenVis = await listen<VisibilityPayload>("pet:visibility-changed", (event) => {
        isVisibleRef.current = event.payload.visible;
        controller.setSuspended(!event.payload.visible || document.hidden);
      });

      unlistenSettings = await listen<PetSettingsPayload>("pet:settings-changed", (event) => {
        const nextSize = Math.min(160, Math.max(100, event.payload.petSize));
        setPetSize(nextSize);
        cuteMomentsRef.current = event.payload.cuteMomentsEnabled;
        controller.setAmbientMomentsEnabled(event.payload.cuteMomentsEnabled);
      });

      window.__pikuDebug = {
        play: (state) => controller.play(state),
        toggleOverlay: () => setShowOverlay((v) => !v),
        triggerBubble: (text) => {
          if (!cuteMomentsRef.current) return;
          if (text) {
            controller.play("wave");
            void invoke("show_bubble", { text, durationMs: 4500 });
          } else {
            controller.triggerAmbientMoment();
          }
        },
        getSnapshot: () => controller.getSnapshot(),
      };
    };

    void init();

    const handleDocVis = () => {
      if (activeController) {
        activeController.setSuspended(document.hidden || !isVisibleRef.current);
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "g") {
        e.preventDefault();
        setShowOverlay((v) => !v);
      }
    };

    document.addEventListener("visibilitychange", handleDocVis);
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      mounted = false;
      document.removeEventListener("visibilitychange", handleDocVis);
      window.removeEventListener("keydown", handleKeyDown);
      unlistenHint?.();
      unlistenVis?.();
      unlistenSettings?.();
      activeController?.stop();
      activeStore?.dispose();
      delete window.__pikuDebug;
    };
  }, []);

  if (!engine) {
    return null;
  }

  const handlePointerDown: React.PointerEventHandler<HTMLCanvasElement> = (e) => {
    if (e.button !== 0) return;

    const rect = e.currentTarget.getBoundingClientRect();
    const u = (e.clientX - rect.left) / rect.width;
    const v = (e.clientY - rect.top) / rect.height;

    // Ignore clicks in transparent empty corners outside Piku's silhouette
    if (!engine.store.hitTest(u, v)) {
      pointerRef.current = null;
      return;
    }

    pointerRef.current = {
      pointerId: e.pointerId,
      startX: e.screenX,
      startY: e.screenY,
      dragged: false,
    };
    isDraggingRef.current = false;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const handlePointerMove: React.PointerEventHandler<HTMLCanvasElement> = (e) => {
    const session = pointerRef.current;
    if (!session || session.pointerId !== e.pointerId) return;

    if (!session.dragged) {
      const dist = Math.hypot(e.screenX - session.startX, e.screenY - session.startY);
      if (dist <= DRAG_THRESHOLD_PX) return;
      session.dragged = true;
      isDraggingRef.current = true;
      engine.controller.startDrag();
      void invoke("begin_pet_drag");
    }

    // Coalesce drag movement to display refresh rate (avoid flooding IPC on high-polling mice)
    if (dragRafRef.current === null) {
      dragRafRef.current = window.requestAnimationFrame(() => {
        dragRafRef.current = null;
        if (isDraggingRef.current) {
          void invoke("move_pet_drag");
        }
      });
    }
  };

  const finishDrag = () => {
    if (dragRafRef.current !== null) {
      window.cancelAnimationFrame(dragRafRef.current);
      dragRafRef.current = null;
    }
    isDraggingRef.current = false;
    engine.controller.endDrag();
    void invoke("end_pet_drag");
  };

  const handlePointerUp: React.PointerEventHandler<HTMLCanvasElement> = (e) => {
    const session = pointerRef.current;
    pointerRef.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    if (!session) return;

    if (session.dragged || isDraggingRef.current) {
      finishDrag();
      return;
    }

    const dist = Math.hypot(e.screenX - session.startX, e.screenY - session.startY);
    if (dist <= DRAG_THRESHOLD_PX) {
      engine.controller.play("click");
      window.setTimeout(() => {
        void invoke("open_greeting_panel");
      }, CLICK_PANEL_DELAY_MS);
    }
  };

  const handlePointerCancel: React.PointerEventHandler<HTMLCanvasElement> = (e) => {
    const wasDragged = pointerRef.current?.dragged || isDraggingRef.current;
    pointerRef.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    if (wasDragged) {
      finishDrag();
    }
  };

  return (
    <div className="pet-window-root">
      <PetCanvas
        controller={engine.controller}
        manifest={engine.store.manifest}
        size={petSize}
        showDevOverlay={showOverlay}
        showShadow
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerCancel}
      />
    </div>
  );
}
