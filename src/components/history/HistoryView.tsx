import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AppWindow,
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  RefreshCw,
  X,
} from "lucide-react";
import type { AppUsage, DailyTotal, DaySummary } from "../../types/tracking";
import {
  computeRelativeBarPercent,
  formatDuration,
} from "../panel/formatters";
import {
  addDaysIso,
  clampIsoToToday,
  dayRelativeBadge,
  firstWeekdayOfMonth,
  formatDayHeading,
  formatDayNumber,
  formatHourLabel,
  formatMonthHeading,
  formatWeekRange,
  formatWeekdayShort,
  getHeatmapTier,
  localTodayIso,
  shiftYearMonth,
  yearMonthOf,
} from "./dateUtils";

export type HistoryTab = "day" | "week" | "month";

interface HistoryViewProps {
  initialDate?: string | null;
  initialTab?: HistoryTab;
  onBackToToday: () => void;
  onClose: () => void;
}

const APP_BAR_COLORS = [
  "var(--color-mint)",
  "var(--color-sky)",
  "var(--color-lavender)",
  "var(--color-peach)",
] as const;

const WEEKDAY_HEADERS = ["S", "M", "T", "W", "T", "F", "S"] as const;

function HistoryAppIcon({ app }: { app: AppUsage }) {
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

function StateCard({
  title,
  text,
  onRetry,
}: {
  title: string;
  text: string;
  onRetry?: () => void;
}) {
  const illustrationSrc = onRetry
    ? "/assets/character/piku/static/error.png"
    : "/assets/character/piku/static/empty_state.png";

  return (
    <div className="history-state-card" role={onRetry ? "alert" : undefined}>
      <img
        src={illustrationSrc}
        alt=""
        className="state-illustration is-compact"
        draggable={false}
      />
      <p className="today-state-title">{title}</p>
      <p className="today-state-text">{text}</p>
      {onRetry && (
        <button type="button" className="btn-secondary today-retry-btn" onClick={onRetry}>
          <RefreshCw size={13} strokeWidth={2.2} />
          <span>Try again</span>
        </button>
      )}
    </div>
  );
}

export function HistoryView({
  initialDate,
  initialTab = "day",
  onBackToToday,
  onClose,
}: HistoryViewProps) {
  const todayIso = localTodayIso();
  const safeInitial = clampIsoToToday(initialDate || todayIso, todayIso);

  const [tab, setTab] = useState<HistoryTab>(initialTab);
  const [selectedDate, setSelectedDate] = useState<string>(safeInitial);
  const [weekEndDate, setWeekEndDate] = useState<string>(todayIso);
  const [monthCursor, setMonthCursor] = useState(() => yearMonthOf(safeInitial));

  const [daySummary, setDaySummary] = useState<DaySummary | null>(null);
  const [weekTotals, setWeekTotals] = useState<DailyTotal[] | null>(null);
  const [monthTotals, setMonthTotals] = useState<DailyTotal[] | null>(null);
  const [loadedDayKey, setLoadedDayKey] = useState<string | null>(null);
  const [loadedWeekKey, setLoadedWeekKey] = useState<string | null>(null);
  const [loadedMonthKey, setLoadedMonthKey] = useState<string | null>(null);

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [hoveredHour, setHoveredHour] = useState<number | null>(null);
  const [hoveredMonthDay, setHoveredMonthDay] = useState<DailyTotal | null>(null);

  const loadCurrentTab = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (tab === "day") {
        const res = await invoke<DaySummary>("get_day_detail", { date: selectedDate });
        setDaySummary(res);
        setLoadedDayKey(selectedDate);
      } else if (tab === "week") {
        const res = await invoke<DailyTotal[]>("get_week_summary", {
          endDate: weekEndDate,
        });
        setWeekTotals(res);
        setLoadedWeekKey(weekEndDate);
      } else {
        const key = `${monthCursor.year}-${monthCursor.month}`;
        const res = await invoke<DailyTotal[]>("get_month_summary", {
          year: monthCursor.year,
          month: monthCursor.month,
        });
        setMonthTotals(res);
        setLoadedMonthKey(key);
      }
    } catch (err) {
      setError(typeof err === "string" ? err : "Could not load local history");
    } finally {
      setLoading(false);
    }
  }, [tab, selectedDate, weekEndDate, monthCursor.year, monthCursor.month]);

  useEffect(() => {
    setHoveredHour(null);
    setHoveredMonthDay(null);
    void loadCurrentTab();
  }, [loadCurrentTab]);

  const handleSwitchTab = (nextTab: HistoryTab) => {
    if (nextTab === tab) return;
    if (nextTab === "week") {
      // Anchor 7-day window so selectedDate is visible while never exceeding today
      const sevenDaysAgo = addDaysIso(todayIso, -6);
      if (selectedDate >= sevenDaysAgo && selectedDate <= todayIso) {
        setWeekEndDate(todayIso);
      } else {
        setWeekEndDate(clampIsoToToday(addDaysIso(selectedDate, 3), todayIso));
      }
    } else if (nextTab === "month") {
      setMonthCursor(yearMonthOf(selectedDate));
    }
    setTab(nextTab);
  };

  const openDateInDayView = (iso: string) => {
    const safe = clampIsoToToday(iso, todayIso);
    setSelectedDate(safe);
    setMonthCursor(yearMonthOf(safe));
    setTab("day");
  };

  // --- Day view derived values ---
  const dayApps = useMemo(() => {
    if (!daySummary) return [];
    return [...daySummary.apps].sort((a, b) => b.activeSecs - a.activeSecs);
  }, [daySummary]);
  const dayMaxAppSecs = dayApps[0]?.activeSecs ?? 0;
  const dayHourly = daySummary?.hourlySecs ?? [];
  const dayMaxHourSecs = useMemo(
    () => dayHourly.reduce((m, v) => (v > m ? v : m), 0),
    [dayHourly]
  );
  const canNextDay = selectedDate < todayIso;
  const dayBadge = dayRelativeBadge(selectedDate, todayIso);

  // --- Week view derived values ---
  const weekStartDate = weekTotals?.[0]?.date ?? addDaysIso(weekEndDate, -6);
  const weekActualEnd = weekTotals?.[weekTotals.length - 1]?.date ?? weekEndDate;
  const weekSumSecs = useMemo(
    () => (weekTotals ?? []).reduce((sum, d) => sum + d.totalSecs, 0),
    [weekTotals]
  );
  const weekMaxSecs = useMemo(
    () => (weekTotals ?? []).reduce((m, d) => (d.totalSecs > m ? d.totalSecs : m), 0),
    [weekTotals]
  );
  const weekActiveDays = useMemo(
    () => (weekTotals ?? []).filter((d) => d.totalSecs > 0).reverse(),
    [weekTotals]
  );
  const canNextWeek = weekEndDate < todayIso;

  // --- Month view derived values ---
  const todayYM = yearMonthOf(todayIso);
  const canNextMonth =
    monthCursor.year < todayYM.year ||
    (monthCursor.year === todayYM.year && monthCursor.month < todayYM.month);
  const monthLeadingBlanks = firstWeekdayOfMonth(monthCursor.year, monthCursor.month);
  const monthSumSecs = useMemo(
    () => (monthTotals ?? []).reduce((sum, d) => sum + d.totalSecs, 0),
    [monthTotals]
  );
  const monthMaxSecs = useMemo(
    () => (monthTotals ?? []).reduce((m, d) => (d.totalSecs > m ? d.totalSecs : m), 0),
    [monthTotals]
  );
  const monthActiveCount = useMemo(
    () => (monthTotals ?? []).filter((d) => d.totalSecs > 0).length,
    [monthTotals]
  );

  const dayRefreshing = loading && loadedDayKey !== selectedDate;
  const weekRefreshing = loading && loadedWeekKey !== weekEndDate;
  const monthKey = `${monthCursor.year}-${monthCursor.month}`;
  const monthRefreshing = loading && loadedMonthKey !== monthKey;

  return (
    <section className="history-card" aria-label="Screen time history">
      <header className="history-header">
        <button
          type="button"
          className="history-back-btn"
          onClick={onBackToToday}
          aria-label="Back to Today"
        >
          <ArrowLeft size={14} strokeWidth={2.2} />
          <span>Today</span>
        </button>

        <h1 className="history-title">Other days</h1>

        <button
          type="button"
          className="today-close-btn"
          aria-label="Close"
          onClick={onClose}
        >
          <X size={16} strokeWidth={2.2} />
        </button>
      </header>

      <nav className="history-tabs" aria-label="History timeframe">
        {(["day", "week", "month"] as const).map((key) => (
          <button
            key={key}
            type="button"
            className={`history-tab-btn${tab === key ? " is-active" : ""}`}
            aria-pressed={tab === key}
            onClick={() => handleSwitchTab(key)}
          >
            {key === "day" ? "Day" : key === "week" ? "Week" : "Month"}
          </button>
        ))}
      </nav>

      {/* ===================== DAY VIEW ===================== */}
      {tab === "day" && (
        <div className={`history-stage${dayRefreshing ? " is-refreshing" : ""}`} key="tab-day">
          <div className="history-nav-bar">
            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Previous day"
              onClick={() => setSelectedDate(addDaysIso(selectedDate, -1))}
            >
              <ChevronLeft size={16} strokeWidth={2.2} />
            </button>

            <div className="history-nav-center">
              <span className="history-nav-label">{formatDayHeading(selectedDate)}</span>
              {dayBadge && <span className="history-nav-pill">{dayBadge}</span>}
            </div>

            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Next day"
              disabled={!canNextDay}
              onClick={() =>
                setSelectedDate(clampIsoToToday(addDaysIso(selectedDate, 1), todayIso))
              }
            >
              <ChevronRight size={16} strokeWidth={2.2} />
            </button>
          </div>

          {loading && !daySummary ? (
            <div className="history-loading-stack" aria-label="Loading day history">
              <div className="history-skeleton-card" />
              <div className="today-skeleton-row" />
              <div className="today-skeleton-row" />
            </div>
          ) : error ? (
            <StateCard
              title="Couldn't open this page"
              text="Piku had a tiny hiccup reading this day's local notes."
              onRetry={() => void loadCurrentTab()}
            />
          ) : (
            <>
              <div className="history-surface-card">
                <div className="history-day-top">
                  <div className="history-day-total">
                    {daySummary && daySummary.totalSecs > 0
                      ? formatDuration(daySummary.totalSecs)
                      : "Quiet day"}
                  </div>
                  <div className="history-hour-readout">
                    {hoveredHour !== null
                      ? `${formatHourLabel(hoveredHour)} · ${
                          (dayHourly[hoveredHour] ?? 0) > 0
                            ? formatDuration(dayHourly[hoveredHour])
                            : "quiet"
                        }`
                      : "24h rhythm"}
                  </div>
                </div>

                <div
                  className="history-hourly-chart"
                  role="img"
                  aria-label="Hourly screen time activity across 24 hours"
                  onMouseLeave={() => setHoveredHour(null)}
                >
                  {Array.from({ length: 24 }, (_, hour) => {
                    const secs = dayHourly[hour] ?? 0;
                    const heightPct =
                      secs > 0 && dayMaxHourSecs > 0
                        ? Math.max(12, Math.round((secs / dayMaxHourSecs) * 100))
                        : 0;
                    const isHovered = hoveredHour === hour;
                    return (
                      <button
                        key={hour}
                        type="button"
                        className={`history-hour-slot${isHovered ? " is-hovered" : ""}`}
                        aria-label={`${formatHourLabel(hour)}: ${
                          secs > 0 ? formatDuration(secs) : "no recorded activity"
                        }`}
                        onMouseEnter={() => setHoveredHour(hour)}
                        onFocus={() => setHoveredHour(hour)}
                        onBlur={() => setHoveredHour(null)}
                      >
                        <span
                          className={`history-hour-bar${secs > 0 ? " has-activity" : ""}`}
                          style={secs > 0 ? { height: `${heightPct}%` } : undefined}
                        />
                      </button>
                    );
                  })}
                </div>

                <div className="history-hour-axis" aria-hidden="true">
                  <span>12a</span>
                  <span>6a</span>
                  <span>12p</span>
                  <span>6p</span>
                  <span>11p</span>
                </div>
              </div>

              <div className="history-list-region">
                {dayApps.length === 0 || !daySummary || daySummary.totalSecs <= 0 ? (
                  <StateCard
                    title="No recorded activity"
                    text="Piku didn't note any screen time on this day — you may have been away, resting, or Piku wasn't open."
                  />
                ) : (
                  <>
                    <div className="today-section-label">
                      <span>Apps visited ({dayApps.length})</span>
                    </div>
                    <ul className="today-app-list history-app-scroll">
                      {dayApps.map((app, idx) => {
                        const pct = computeRelativeBarPercent(app.activeSecs, dayMaxAppSecs);
                        const color = APP_BAR_COLORS[idx] ?? APP_BAR_COLORS[3];
                        return (
                          <li key={app.appId} className="today-app-row is-compact">
                            <HistoryAppIcon app={app} />
                            <div className="today-app-main">
                              <div className="today-app-meta">
                                <span className="today-app-name" title={app.displayName}>
                                  {app.displayName}
                                </span>
                                <span className="today-app-time">
                                  {formatDuration(app.activeSecs)}
                                </span>
                              </div>
                              <div className="today-bar-track" aria-hidden="true">
                                <div
                                  className="today-bar-fill"
                                  style={{ width: `${pct}%`, backgroundColor: color }}
                                />
                              </div>
                            </div>
                          </li>
                        );
                      })}
                    </ul>
                  </>
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* ===================== WEEK VIEW ===================== */}
      {tab === "week" && (
        <div className={`history-stage${weekRefreshing ? " is-refreshing" : ""}`} key="tab-week">
          <div className="history-nav-bar">
            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Previous 7 days"
              onClick={() => setWeekEndDate(addDaysIso(weekEndDate, -7))}
            >
              <ChevronLeft size={16} strokeWidth={2.2} />
            </button>

            <div className="history-nav-center">
              <span className="history-nav-label">
                {formatWeekRange(weekStartDate, weekActualEnd)}
              </span>
            </div>

            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Next 7 days"
              disabled={!canNextWeek}
              onClick={() =>
                setWeekEndDate(clampIsoToToday(addDaysIso(weekEndDate, 7), todayIso))
              }
            >
              <ChevronRight size={16} strokeWidth={2.2} />
            </button>
          </div>

          {loading && !weekTotals ? (
            <div className="history-loading-stack" aria-label="Loading week history">
              <div className="history-skeleton-card is-tall" />
              <div className="today-skeleton-row" />
            </div>
          ) : error ? (
            <StateCard
              title="Couldn't peek at this week"
              text="Piku had trouble reading your weekly notes."
              onRetry={() => void loadCurrentTab()}
            />
          ) : (
            <>
              <div className="history-surface-card">
                <div className="history-day-top">
                  <div>
                    <div className="history-day-total">
                      {weekSumSecs > 0 ? formatDuration(weekSumSecs) : "Quiet week"}
                    </div>
                    <p className="history-sub-caption">
                      {weekSumSecs > 0
                        ? `Recorded across ${weekActiveDays.length} ${
                            weekActiveDays.length === 1 ? "day" : "days"
                          }`
                        : "No recorded activity in these 7 days"}
                    </p>
                  </div>
                  <span className="history-tap-hint">Tap a day</span>
                </div>

                <div className="history-week-bars" role="list" aria-label="7-day screen time">
                  {(weekTotals ?? []).map((day) => {
                    const isSelected = day.date === selectedDate;
                    const hasActivity = day.totalSecs > 0;
                    const heightPct =
                      hasActivity && weekMaxSecs > 0
                        ? Math.max(14, Math.round((day.totalSecs / weekMaxSecs) * 100))
                        : 0;
                    return (
                      <button
                        key={day.date}
                        type="button"
                        role="listitem"
                        className={`history-week-col${isSelected ? " is-selected" : ""}`}
                        aria-label={`${formatDayHeading(day.date)}: ${
                          hasActivity ? formatDuration(day.totalSecs) : "no recorded activity"
                        }`}
                        onClick={() => openDateInDayView(day.date)}
                      >
                        <span className="history-week-val">
                          {hasActivity ? formatDuration(day.totalSecs) : "·"}
                        </span>
                        <span className="history-week-bar-slot">
                          <span
                            className={`history-week-bar${
                              hasActivity ? " has-activity" : ""
                            }${isSelected ? " is-selected" : ""}`}
                            style={hasActivity ? { height: `${heightPct}%` } : undefined}
                          />
                        </span>
                        <span className="history-week-dow">{formatWeekdayShort(day.date)}</span>
                        <span className="history-week-num">{formatDayNumber(day.date)}</span>
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="history-list-region">
                {weekActiveDays.length === 0 ? (
                  <StateCard
                    title="A quiet stretch of days"
                    text="Tap any day above to inspect it, or wander back to an earlier week."
                  />
                ) : (
                  <>
                    <div className="today-section-label">
                      <span>Days with notes</span>
                    </div>
                    <ul className="history-week-list">
                      {weekActiveDays.map((day) => {
                        const isSelected = day.date === selectedDate;
                        return (
                          <li key={day.date}>
                            <button
                              type="button"
                              className={`history-day-pill-row${isSelected ? " is-selected" : ""}`}
                              onClick={() => openDateInDayView(day.date)}
                            >
                              <span className="history-day-pill-date">
                                {formatDayHeading(day.date)}
                                {day.date === todayIso && (
                                  <span className="history-inline-tag">Today</span>
                                )}
                              </span>
                              <span className="history-day-pill-right">
                                <span className="today-app-time">
                                  {formatDuration(day.totalSecs)}
                                </span>
                                <ChevronRight size={14} strokeWidth={2} />
                              </span>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </>
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* ===================== MONTH VIEW ===================== */}
      {tab === "month" && (
        <div className={`history-stage${monthRefreshing ? " is-refreshing" : ""}`} key="tab-month">
          <div className="history-nav-bar">
            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Previous month"
              onClick={() =>
                setMonthCursor(shiftYearMonth(monthCursor.year, monthCursor.month, -1))
              }
            >
              <ChevronLeft size={16} strokeWidth={2.2} />
            </button>

            <div className="history-nav-center">
              <span className="history-nav-label">
                {formatMonthHeading(monthCursor.year, monthCursor.month)}
              </span>
            </div>

            <button
              type="button"
              className="history-nav-arrow"
              aria-label="Next month"
              disabled={!canNextMonth}
              onClick={() =>
                setMonthCursor(shiftYearMonth(monthCursor.year, monthCursor.month, 1))
              }
            >
              <ChevronRight size={16} strokeWidth={2.2} />
            </button>
          </div>

          {loading && !monthTotals ? (
            <div className="history-loading-stack" aria-label="Loading month calendar">
              <div className="history-skeleton-card is-calendar" />
            </div>
          ) : error ? (
            <StateCard
              title="Couldn't load this month"
              text="Piku couldn't open the monthly calendar right now."
              onRetry={() => void loadCurrentTab()}
            />
          ) : (
            <>
              <div className="history-surface-card history-month-card">
                <div className="history-cal-weekdays" aria-hidden="true">
                  {WEEKDAY_HEADERS.map((d, i) => (
                    <span key={i} className="history-cal-weekday">
                      {d}
                    </span>
                  ))}
                </div>

                <div
                  className="history-cal-grid"
                  role="grid"
                  aria-label={formatMonthHeading(monthCursor.year, monthCursor.month)}
                  onMouseLeave={() => setHoveredMonthDay(null)}
                >
                  {Array.from({ length: monthLeadingBlanks }, (_, i) => (
                    <span key={`blank-${i}`} className="history-cal-blank" aria-hidden="true" />
                  ))}

                  {(monthTotals ?? []).map((day) => {
                    const isFuture = day.date > todayIso;
                    const isSelected = day.date === selectedDate;
                    const isToday = day.date === todayIso;
                    const tier = isFuture ? 0 : getHeatmapTier(day.totalSecs, monthMaxSecs);
                    return (
                      <button
                        key={day.date}
                        type="button"
                        disabled={isFuture}
                        className={`history-cal-cell tier-${tier}${
                          isSelected ? " is-selected" : ""
                        }${isToday ? " is-today" : ""}${isFuture ? " is-future" : ""}`}
                        aria-label={`${formatDayHeading(day.date)}: ${
                          isFuture
                            ? "upcoming date"
                            : day.totalSecs > 0
                              ? formatDuration(day.totalSecs)
                              : "no recorded activity"
                        }`}
                        onMouseEnter={() => !isFuture && setHoveredMonthDay(day)}
                        onFocus={() => !isFuture && setHoveredMonthDay(day)}
                        onBlur={() => setHoveredMonthDay(null)}
                        onClick={() => openDateInDayView(day.date)}
                      >
                        <span className="history-cal-daynum">{formatDayNumber(day.date)}</span>
                      </button>
                    );
                  })}
                </div>

                <div className="history-cal-caption">
                  {hoveredMonthDay ? (
                    <span>
                      {formatDayHeading(hoveredMonthDay.date)} ·{" "}
                      <strong>
                        {hoveredMonthDay.totalSecs > 0
                          ? formatDuration(hoveredMonthDay.totalSecs)
                          : "No recorded activity"}
                      </strong>
                    </span>
                  ) : (
                    <span>Tap any day to open its journal page</span>
                  )}
                </div>
              </div>

              <div className="history-month-footer">
                <div className="history-month-summary">
                  {monthSumSecs > 0 ? (
                    <>
                      <strong>{formatDuration(monthSumSecs)}</strong> across{" "}
                      {monthActiveCount} {monthActiveCount === 1 ? "day" : "days"}
                    </>
                  ) : (
                    <span>No recorded activity this month</span>
                  )}
                </div>

                <div className="history-legend" aria-label="Activity intensity legend">
                  <span className="history-legend-label">No notes</span>
                  <span className="history-legend-swatch tier-0" />
                  <span className="history-legend-swatch tier-1" />
                  <span className="history-legend-swatch tier-2" />
                  <span className="history-legend-swatch tier-3" />
                  <span className="history-legend-swatch tier-4" />
                  <span className="history-legend-label">More</span>
                </div>
              </div>
            </>
          )}
        </div>
      )}
    </section>
  );
}
