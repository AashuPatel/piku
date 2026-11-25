import { useState } from "react";
import { AppWindow, ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
import type { AnimationController } from "../../animation/controller";
import type { SpriteStore } from "../../animation/spriteStore";
import type { AppUsage, DaySummary } from "../../types/tracking";
import { PetCanvas } from "../pet/PetCanvas";
import {
  computeRelativeBarPercent,
  formatDuration,
  formatFriendlyDate,
} from "./formatters";

interface TodayViewProps {
  summary: DaySummary | null;
  loading: boolean;
  error: string | null;
  expanded: boolean;
  engine: {
    store: SpriteStore;
    controller: AnimationController;
  } | null;
  onToggleExpand: () => void;
  onRetry: () => void;
  onOpenHistory: () => void;
  onClose: () => void;
}

const TOP_BAR_COLORS = [
  "var(--color-mint)",
  "var(--color-sky)",
  "var(--color-lavender)",
] as const;

const EXTRA_BAR_COLOR = "var(--color-peach)";

function AppIcon({ app }: { app: AppUsage }) {
  const [imgFailed, setImgFailed] = useState(false);
  const initial = app.displayName.trim().charAt(0).toUpperCase();

  if (app.iconDataUrl && !imgFailed) {
    return (
      <span className="today-app-icon-wrap">
        <img
          src={app.iconDataUrl}
          alt=""
          className="today-app-icon-img"
          draggable={false}
          onError={() => setImgFailed(true)}
        />
      </span>
    );
  }

  return (
    <span className="today-app-icon-wrap today-app-icon-fallback" aria-hidden="true">
      {initial ? (
        <span className="today-app-icon-letter">{initial}</span>
      ) : (
        <AppWindow size={14} strokeWidth={2} />
      )}
    </span>
  );
}

function AppUsageRow({
  app,
  maxSecs,
  barColor,
  compact = false,
}: {
  app: AppUsage;
  maxSecs: number;
  barColor: string;
  compact?: boolean;
}) {
  const percent = computeRelativeBarPercent(app.activeSecs, maxSecs);

  return (
    <li className={`today-app-row${compact ? " is-compact" : ""}`}>
      <AppIcon app={app} />
      <div className="today-app-main">
        <div className="today-app-meta">
          <span className="today-app-name" title={app.displayName}>
            {app.displayName}
          </span>
          <span className="today-app-time">{formatDuration(app.activeSecs)}</span>
        </div>
        <div className="today-bar-track" aria-hidden="true">
          <div
            className="today-bar-fill"
            style={{
              width: `${percent}%`,
              backgroundColor: barColor,
            }}
          />
        </div>
      </div>
    </li>
  );
}

export function TodayView({
  summary,
  loading,
  error,
  expanded,
  engine,
  onToggleExpand,
  onRetry,
  onOpenHistory,
  onClose,
}: TodayViewProps) {
  const rankedApps = summary
    ? [...summary.apps].sort((a, b) => b.activeSecs - a.activeSecs)
    : [];
  const topApps = rankedApps.slice(0, 3);
  const extraApps = rankedApps.slice(3);
  const maxSecs = rankedApps[0]?.activeSecs ?? 0;
  const isEmpty = !loading && !error && (!summary || summary.totalSecs <= 0 || rankedApps.length === 0);
  const dateLabel = formatFriendlyDate(summary?.date);

  return (
    <section className="today-card" aria-label="Today's screen time">
      <header className="today-header">
        <div className="today-header-text">
          <h1 className="today-title">Your little day</h1>
          <p className="today-date">{dateLabel}</p>
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

      <div className="today-hero">
        <div className="today-hero-copy">
          {loading && !summary ? (
            <>
              <div className="today-skeleton-hero" />
              <div className="today-skeleton-sub" />
            </>
          ) : (
            <>
              <div className="today-hero-time">
                {summary ? formatDuration(summary.totalSecs) : "0m"}
              </div>
              <p className="today-hero-subtitle">
                {isEmpty
                  ? "A quiet start to your day."
                  : "A little time spent in your digital world."}
              </p>
            </>
          )}
        </div>

        <div className="today-hero-piku" aria-hidden="true">
          <svg className="today-hero-doodle" viewBox="0 0 24 24" fill="none">
            <path
              d="M12 3.5L13.3 8.2L18 9.5L13.3 10.8L12 15.5L10.7 10.8L6 9.5L10.7 8.2L12 3.5Z"
              fill="var(--color-lime)"
            />
            <circle cx="19" cy="17.5" r="2" fill="var(--color-mint)" />
          </svg>
          {engine && (
            <PetCanvas
              controller={engine.controller}
              manifest={engine.store.manifest}
              size={58}
              showShadow={false}
            />
          )}
        </div>
      </div>

      <div className="today-body">
        {loading && !summary ? (
          <div className="today-loading-list" aria-label="Loading today's apps">
            <div className="today-skeleton-row" />
            <div className="today-skeleton-row" />
            <div className="today-skeleton-row" />
          </div>
        ) : error ? (
          <div className="today-state-box" role="alert">
            <img
              src="/assets/character/piku/static/error.png"
              alt=""
              className="state-illustration"
              draggable={false}
            />
            <p className="today-state-title">Couldn&apos;t peek at today&apos;s notes</p>
            <p className="today-state-text">
              Piku had a tiny hiccup reading local usage data.
            </p>
            <button type="button" className="btn-secondary today-retry-btn" onClick={onRetry}>
              <RefreshCw size={13} strokeWidth={2.2} />
              <span>Try again</span>
            </button>
          </div>
        ) : isEmpty ? (
          <div className="today-state-box">
            <img
              src="/assets/character/piku/static/empty_state.png"
              alt=""
              className="state-illustration"
              draggable={false}
            />
            <p className="today-state-title">Still quiet here today</p>
            <p className="today-state-text">
              As you spend time in your apps, Piku will gently jot them down in your little journal.
            </p>
          </div>
        ) : (
          <div className="today-apps-section">
            <div className="today-section-label">
              <span>Most visited today</span>
            </div>

            <ul className="today-app-list">
              {topApps.map((app, index) => (
                <AppUsageRow
                  key={app.appId}
                  app={app}
                  maxSecs={maxSecs}
                  barColor={TOP_BAR_COLORS[index] ?? EXTRA_BAR_COLOR}
                />
              ))}
            </ul>

            {extraApps.length > 0 && (
              <div className="today-more-wrap">
                <button
                  type="button"
                  className="today-expand-btn"
                  aria-expanded={expanded}
                  onClick={onToggleExpand}
                >
                  <span>
                    {expanded
                      ? "Show fewer apps"
                      : `See all apps (${rankedApps.length})`}
                  </span>
                  <ChevronDown
                    size={14}
                    strokeWidth={2.2}
                    className={`today-expand-chevron${expanded ? " is-open" : ""}`}
                  />
                </button>

                <div className={`today-extra-collapse${expanded ? " is-open" : ""}`}>
                  <div className="today-extra-inner">
                    <ul className="today-app-list today-extra-list">
                      {extraApps.map((app) => (
                        <AppUsageRow
                          key={app.appId}
                          app={app}
                          maxSecs={maxSecs}
                          barColor={EXTRA_BAR_COLOR}
                          compact
                        />
                      ))}
                    </ul>
                  </div>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <footer className="today-footer">
        <button
          type="button"
          className="today-history-link"
          onClick={onOpenHistory}
        >
          <span>Want to wander through other days?</span>
          <ChevronRight size={14} strokeWidth={2} />
        </button>
      </footer>
    </section>
  );
}
