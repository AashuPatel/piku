import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import { AnimationController } from "../../animation/controller";
import { SpriteStore } from "../../animation/spriteStore";
import type { AnimationStateName } from "../../animation/types";
import type { DaySummary } from "../../types/tracking";
import { HistoryView } from "../history/HistoryView";
import { PetCanvas } from "../pet/PetCanvas";
import { PreferencesView } from "./PreferencesView";
import { TodayView } from "./TodayView";
import { formatGreetingDuration } from "./formatters";

type PanelMode = "greeting" | "today" | "history" | "preferences";

const PANEL_SPRITE_STATES: readonly AnimationStateName[] = [
  "idle",
  "blink",
  "wave",
  "happy",
];

function modeNeedsPetVisual(mode: PanelMode): boolean {
  return mode === "greeting" || mode === "today";
}

const GREETING_SIZE = { width: 348, height: 268 };
const TODAY_COMPACT_SIZE = { width: 380, height: 472 };
const TODAY_EXPANDED_SIZE = { width: 380, height: 584 };
const HISTORY_SIZE = { width: 412, height: 576 };
const PREFERENCES_SIZE = { width: 368, height: 428 };

function sizeForState(mode: PanelMode, expandedAllApps: boolean) {
  if (mode === "greeting") return GREETING_SIZE;
  if (mode === "preferences") return PREFERENCES_SIZE;
  if (mode === "history") return HISTORY_SIZE;
  return expandedAllApps ? TODAY_EXPANDED_SIZE : TODAY_COMPACT_SIZE;
}

function getInitialMode(): PanelMode {
  const view = new URLSearchParams(window.location.search).get("view");
  if (view === "today" || view === "history" || view === "preferences") return view;
  return "greeting";
}

function timeGreeting(): string {
  const hour = new Date().getHours();
  if (hour >= 22 || hour < 5) return "It's getting late, sleepyhead 🌙";
  if (hour < 12) return "Hehe, good morning! ♡";
  if (hour < 18) return "Hehe, you noticed me ♡";
  return "Cozy evening, isn't it? ♡";
}

export function GreetingPanel() {
  const [mode, setMode] = useState<PanelMode>(getInitialMode);
  const [expandedAllApps, setExpandedAllApps] = useState(false);
  const lastSizeRef = useRef(sizeForState(getInitialMode(), false));
  const [summary, setSummary] = useState<DaySummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [engine, setEngine] = useState<{
    store: SpriteStore;
    controller: AnimationController;
  } | null>(null);
  const [greeting, setGreeting] = useState(timeGreeting);

  const mountedRef = useRef(true);
  const modeRef = useRef<PanelMode>(mode);
  modeRef.current = mode;
  const engineRef = useRef<{
    store: SpriteStore;
    controller: AnimationController;
  } | null>(null);
  const loadingEngineRef = useRef<Promise<void> | null>(null);
  const pendingPlayRef = useRef<"wave" | "happy" | null>(
    getInitialMode() === "greeting" ? "wave" : null
  );

  const fetchToday = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<DaySummary>("get_today_summary");
      setSummary(data);
    } catch (err) {
      setError(typeof err === "string" ? err : "Failed to load today's summary");
    } finally {
      setLoading(false);
    }
  }, []);

  const ensureEngineLoaded = useCallback(() => {
    if (engineRef.current || loadingEngineRef.current || !mountedRef.current) {
      return;
    }

    const loadTask = (async () => {
      try {
        const store = await SpriteStore.load({
          states: PANEL_SPRITE_STATES,
          buildHitMask: false,
        });
        if (!mountedRef.current) {
          store.dispose();
          return;
        }
        const controller = new AnimationController(store, {
          enableRandomBlink: true,
          enableAmbientMoments: false,
          initialBaseState: "idle",
        });

        const nextEngine = { store, controller };
        engineRef.current = nextEngine;
        setEngine(nextEngine);

        if (modeNeedsPetVisual(modeRef.current) && !document.hidden) {
          controller.start();
          if (pendingPlayRef.current) {
            controller.play(pendingPlayRef.current);
            pendingPlayRef.current = null;
          }
        } else {
          controller.stop();
        }
      } catch (err) {
        console.warn("[Piku GreetingPanel] Failed to load panel sprites:", err);
      } finally {
        loadingEngineRef.current = null;
      }
    })();

    loadingEngineRef.current = loadTask;
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    let unlistenOpened: (() => void) | null = null;
    let unlistenNavigate: (() => void) | null = null;

    const init = async () => {
      void fetchToday();

      unlistenOpened = await listen("panel:opened", () => {
        const nextMode = getInitialMode();
        setGreeting(timeGreeting());
        setMode(nextMode);
        setExpandedAllApps(false);
        void fetchToday();
        if (modeNeedsPetVisual(nextMode)) {
          pendingPlayRef.current = nextMode === "greeting" ? "wave" : null;
          if (engineRef.current && !document.hidden) {
            engineRef.current.controller.setSuspended(false);
            if (pendingPlayRef.current) {
              engineRef.current.controller.play(pendingPlayRef.current);
              pendingPlayRef.current = null;
            }
          }
        }
      });

      unlistenNavigate = await listen<{ view: PanelMode }>("panel:navigate", (event) => {
        const nextView = event.payload.view;
        if (
          nextView === "greeting" ||
          nextView === "today" ||
          nextView === "history" ||
          nextView === "preferences"
        ) {
          setExpandedAllApps(false);
          setMode(nextView);
        }
      });
    };

    void init();

    const handleVis = () => {
      const controller = engineRef.current?.controller;
      if (!controller) return;
      controller.setSuspended(document.hidden || !modeNeedsPetVisual(modeRef.current));
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void invoke("close_panel");
      }
    };

    document.addEventListener("visibilitychange", handleVis);
    window.addEventListener("keydown", handleKey);

    return () => {
      mountedRef.current = false;
      document.removeEventListener("visibilitychange", handleVis);
      window.removeEventListener("keydown", handleKey);
      unlistenOpened?.();
      unlistenNavigate?.();
      engineRef.current?.controller.stop();
      engineRef.current?.store.dispose();
      engineRef.current = null;
      loadingEngineRef.current = null;
    };
  }, [fetchToday]);

  useEffect(() => {
    const needsVisual = modeNeedsPetVisual(mode);

    if (needsVisual && !engineRef.current) {
      ensureEngineLoaded();
    }

    const controller = engineRef.current?.controller;
    if (!controller) return;

    const shouldSuspend = !needsVisual || document.hidden;
    controller.setSuspended(shouldSuspend);

    if (!shouldSuspend && pendingPlayRef.current) {
      controller.play(pendingPlayRef.current);
      pendingPlayRef.current = null;
    }
  }, [mode, ensureEngineLoaded]);

  useEffect(() => {
    const target = sizeForState(mode, expandedAllApps);
    if (
      target.width === lastSizeRef.current.width &&
      target.height === lastSizeRef.current.height
    ) {
      return;
    }
    lastSizeRef.current = target;
    void invoke("resize_panel", target);
  }, [mode, expandedAllApps]);

  const handleClose = () => {
    void invoke("close_panel");
  };

  const handleOpenToday = () => {
    if (engineRef.current) {
      engineRef.current.controller.play("happy");
    } else {
      pendingPlayRef.current = "happy";
    }
    setExpandedAllApps(false);
    setMode("today");
    void fetchToday();
  };

  const handleOpenHistory = () => {
    setMode("history");
  };

  const handleBackToToday = () => {
    setExpandedAllApps(false);
    setMode("today");
    void fetchToday();
  };

  const handleToggleExpand = () => {
    setExpandedAllApps((prev) => !prev);
  };

  if (mode === "preferences") {
    return (
      <main className="panel-shell">
        <PreferencesView onClose={handleClose} />
      </main>
    );
  }

  if (mode === "history") {
    return (
      <main className="panel-shell">
        <HistoryView
          initialDate={summary?.date}
          onBackToToday={handleBackToToday}
          onClose={handleClose}
        />
      </main>
    );
  }

  if (mode === "today") {
    return (
      <main className="panel-shell">
        <TodayView
          summary={summary}
          loading={loading}
          error={error}
          expanded={expandedAllApps}
          engine={engine}
          onToggleExpand={handleToggleExpand}
          onRetry={() => void fetchToday()}
          onOpenHistory={handleOpenHistory}
          onClose={handleClose}
        />
      </main>
    );
  }

  return (
    <main className="panel-shell">
      <section className="panel-card">
        <button
          type="button"
          className="panel-close-btn"
          aria-label="Close"
          onClick={handleClose}
        >
          <X size={16} strokeWidth={2.2} />
        </button>

        <div className="panel-pet-stage">
          {engine && (
            <PetCanvas
              controller={engine.controller}
              manifest={engine.store.manifest}
              size={84}
              showShadow={false}
            />
          )}
        </div>

        <h1 className="panel-heading">{greeting}</h1>
        <p className="panel-body">
          {loading && !summary ? (
            "Checking your little day..."
          ) : summary && summary.totalSecs > 0 ? (
            <>
              You&apos;ve been here for{" "}
              <strong className="panel-time-highlight">
                {formatGreetingDuration(summary.totalSecs)}
              </strong>{" "}
              today. Want to see where it went?
            </>
          ) : (
            "A quiet little day so far. Want to peek inside your journal?"
          )}
        </p>

        <div className="panel-actions">
          <button type="button" className="btn-primary" onClick={handleOpenToday}>
            Show me ♡
          </button>
          <button type="button" className="btn-secondary" onClick={handleClose}>
            Maybe later
          </button>
        </div>
      </section>
    </main>
  );
}
