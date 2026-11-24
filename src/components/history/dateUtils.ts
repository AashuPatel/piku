export function toIsoDate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function localTodayIso(): string {
  return toIsoDate(new Date());
}

export function parseIsoDate(iso: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(iso)) return null;
  const [y, m, d] = iso.split("-").map(Number);
  const parsed = new Date(y, m - 1, d);
  if (
    parsed.getFullYear() !== y ||
    parsed.getMonth() !== m - 1 ||
    parsed.getDate() !== d
  ) {
    return null;
  }
  return parsed;
}

export function addDaysIso(iso: string, deltaDays: number): string {
  const base = parseIsoDate(iso) ?? new Date();
  const next = new Date(base.getFullYear(), base.getMonth(), base.getDate() + deltaDays);
  return toIsoDate(next);
}

export function clampIsoToToday(iso: string, todayIso: string): string {
  if (!parseIsoDate(iso)) return todayIso;
  return iso > todayIso ? todayIso : iso;
}

export function yearMonthOf(iso: string): { year: number; month: number } {
  const d = parseIsoDate(iso) ?? new Date();
  return { year: d.getFullYear(), month: d.getMonth() + 1 };
}

export function shiftYearMonth(
  year: number,
  month: number,
  deltaMonths: number
): { year: number; month: number } {
  const d = new Date(year, month - 1 + deltaMonths, 1);
  return { year: d.getFullYear(), month: d.getMonth() + 1 };
}

export function firstWeekdayOfMonth(year: number, month: number): number {
  return new Date(year, month - 1, 1).getDay(); // 0 = Sun .. 6 = Sat
}

export function formatDayHeading(iso: string): string {
  const d = parseIsoDate(iso);
  if (!d) return iso;
  return new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
  }).format(d);
}

export function formatWeekRange(startIso: string, endIso: string): string {
  const s = parseIsoDate(startIso);
  const e = parseIsoDate(endIso);
  if (!s || !e) return `${startIso} – ${endIso}`;

  const sameMonth = s.getFullYear() === e.getFullYear() && s.getMonth() === e.getMonth();
  const startFmt = new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
  }).format(s);
  const endFmt = sameMonth
    ? String(e.getDate())
    : new Intl.DateTimeFormat("en-US", {
        month: "short",
        day: "numeric",
      }).format(e);

  return `${startFmt} – ${endFmt}`;
}

export function formatMonthHeading(year: number, month: number): string {
  const d = new Date(year, month - 1, 1);
  return new Intl.DateTimeFormat("en-US", {
    month: "long",
    year: "numeric",
  }).format(d);
}

export function formatWeekdayShort(iso: string): string {
  const d = parseIsoDate(iso);
  if (!d) return "";
  return new Intl.DateTimeFormat("en-US", { weekday: "short" }).format(d);
}

export function formatDayNumber(iso: string): string {
  const d = parseIsoDate(iso);
  return d ? String(d.getDate()) : "";
}

export function formatHourLabel(hour: number): string {
  const h = ((hour % 24) + 24) % 24;
  if (h === 0) return "12 AM";
  if (h < 12) return `${h} AM`;
  if (h === 12) return "12 PM";
  return `${h - 12} PM`;
}

export function dayRelativeBadge(iso: string, todayIso: string): string | null {
  if (iso === todayIso) return "Today";
  if (iso === addDaysIso(todayIso, -1)) return "Yesterday";
  return null;
}

/**
 * Returns heatmap intensity tier 0..4.
 * Tier 0 strictly means no recorded activity (distinct from active tiers 1..4).
 */
export function getHeatmapTier(totalSecs: number, maxSecs: number): 0 | 1 | 2 | 3 | 4 {
  if (totalSecs <= 0) return 0;
  if (maxSecs <= 0) return 1;
  const ratio = totalSecs / maxSecs;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.5) return 2;
  if (ratio <= 0.78) return 3;
  return 4;
}
