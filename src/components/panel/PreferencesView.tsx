import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Clock, Maximize2, MessageCircleHeart, Power, X } from "lucide-react";
import type { AppSettings, AppSettingsPatch } from "../../types/tracking";

interface PreferencesViewProps {
  onClose: () => void;
}

const PET_SIZE_PRESETS = [
  { label: "Small", value: 100 },
  { label: "Normal", value: 128 },
  { label: "Large", value: 160 },
] as const;

function nearestPresetSize(px: number): 100 | 128 | 160 {
  if (px <= 114) return 100;
  if (px >= 149) return 160;
  return 128;
}

export function PreferencesView({ onClose }: PreferencesViewProps) {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadSettings = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<AppSettings>("get_settings");
      setSettings(data);
    } catch (err) {
      setError(typeof err === "string" ? err : "Could not load preferences");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadSettings();
    let unlisten: (() => void) | null = null;
    void listen<AppSettings>("settings:changed", (event) => {
      setSettings(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [loadSettings]);

  const applyPatch = async (key: string, patch: AppSettingsPatch) => {
    setSavingKey(key);
    setError(null);
    try {
      const updated = await invoke<AppSettings>("update_settings", { patch });
      setSettings(updated);
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't save that preference");
    } finally {
      setSavingKey(null);
    }
  };

  const activeSizePreset = settings ? nearestPresetSize(settings.petSize) : 128;

  return (
    <section className="prefs-card" aria-label="Piku preferences">
      <header className="prefs-header">
        <div>
          <h1 className="prefs-title">Preferences</h1>
          <p className="prefs-subtitle">Little comforts for Piku</p>
        </div>
        <button
          type="button"
          className="today-close-btn"
          aria-label="Close"
          onClick={onClose}
        >
          <X size={16} strokeWidth={2.2} />
        </button>
      </header>

      {loading && !settings ? (
        <div className="prefs-surface">
          <div className="today-skeleton-row" />
          <div className="today-skeleton-row" />
          <div className="today-skeleton-row" />
          <div className="today-skeleton-row" />
        </div>
      ) : settings ? (
        <div className="prefs-surface">
          {/* 1. Tracking */}
          <div className="prefs-row">
            <span className="prefs-icon-badge" aria-hidden="true">
              <Clock size={15} strokeWidth={2} />
            </span>
            <div className="prefs-row-copy">
              <span className="prefs-row-label">Screen-time tracking</span>
              <span className="prefs-row-hint">
                {settings.trackingPaused
                  ? "Paused — Piku is napping"
                  : "Quietly noting your day"}
              </span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={!settings.trackingPaused}
              aria-label="Screen-time tracking"
              disabled={savingKey === "tracking"}
              className={`prefs-switch${!settings.trackingPaused ? " is-on" : ""}`}
              onClick={() =>
                void applyPatch("tracking", {
                  trackingPaused: !settings.trackingPaused,
                })
              }
            >
              <span className="prefs-switch-thumb" />
            </button>
          </div>

          {/* 2. Pet size */}
          <div className="prefs-row prefs-row-stacked">
            <div className="prefs-row-top">
              <span className="prefs-icon-badge" aria-hidden="true">
                <Maximize2 size={14} strokeWidth={2} />
              </span>
              <div className="prefs-row-copy">
                <span className="prefs-row-label">Pet size</span>
                <span className="prefs-row-hint">Choose how big Piku sits on your desktop</span>
              </div>
            </div>
            <div className="prefs-size-group" role="radiogroup" aria-label="Pet size">
              {PET_SIZE_PRESETS.map((preset) => {
                const selected = activeSizePreset === preset.value;
                return (
                  <button
                    key={preset.value}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    disabled={savingKey === "size"}
                    className={`prefs-size-btn${selected ? " is-active" : ""}`}
                    onClick={() =>
                      void applyPatch("size", { petSize: preset.value })
                    }
                  >
                    <span>{preset.label}</span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* 3. Launch on startup */}
          <div className="prefs-row">
            <span className="prefs-icon-badge" aria-hidden="true">
              <Power size={15} strokeWidth={2} />
            </span>
            <div className="prefs-row-copy">
              <span className="prefs-row-label">Launch on startup</span>
              <span className="prefs-row-hint">Wake up with Windows</span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={settings.launchOnStartup}
              aria-label="Launch on startup"
              disabled={savingKey === "startup"}
              className={`prefs-switch${settings.launchOnStartup ? " is-on" : ""}`}
              onClick={() =>
                void applyPatch("startup", {
                  launchOnStartup: !settings.launchOnStartup,
                })
              }
            >
              <span className="prefs-switch-thumb" />
            </button>
          </div>

          {/* 4. Pet interactions */}
          <div className="prefs-row">
            <span className="prefs-icon-badge" aria-hidden="true">
              <MessageCircleHeart size={15} strokeWidth={2} />
            </span>
            <div className="prefs-row-copy">
              <span className="prefs-row-label">Pet interactions</span>
              <span className="prefs-row-hint">Speech bubbles &amp; cute moments</span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={settings.cuteMomentsEnabled}
              aria-label="Pet interactions"
              disabled={savingKey === "interactions"}
              className={`prefs-switch${settings.cuteMomentsEnabled ? " is-on" : ""}`}
              onClick={() =>
                void applyPatch("interactions", {
                  cuteMomentsEnabled: !settings.cuteMomentsEnabled,
                })
              }
            >
              <span className="prefs-switch-thumb" />
            </button>
          </div>
        </div>
      ) : (
        <div className="today-state-box" role="alert">
          <p className="today-state-title">Couldn&apos;t open preferences</p>
          <p className="today-state-text">{error ?? "Something went wrong."}</p>
          <button
            type="button"
            className="btn-secondary today-retry-btn"
            onClick={() => void loadSettings()}
          >
            Try again
          </button>
        </div>
      )}

      {error && settings && (
        <p className="prefs-error-note" role="alert">
          {error}
        </p>
      )}

      <footer className="prefs-footer">
        <span>Everything stays local on your PC</span>
      </footer>
    </section>
  );
}
