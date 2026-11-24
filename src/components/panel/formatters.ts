export function formatDuration(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  if (secs === 0) return "0m";
  if (secs < 60) return `${secs}s`;

  const totalMins = Math.floor(secs / 60);
  const hours = Math.floor(totalMins / 60);
  const mins = totalMins % 60;

  if (hours === 0) return `${mins}m`;
  if (mins === 0) return `${hours}h`;
  return `${hours}h ${mins}m`;
}

export function formatGreetingDuration(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  if (secs < 60) {
    return secs === 1 ? "1 second" : `${secs} seconds`;
  }

  const totalMins = Math.floor(secs / 60);
  const hours = Math.floor(totalMins / 60);
  const mins = totalMins % 60;

  const hPart = hours === 1 ? "1 hour" : `${hours} hours`;
  const mPart = mins === 1 ? "1 minute" : `${mins} minutes`;

  if (hours === 0) return mPart;
  if (mins === 0) return hPart;
  return `${hPart} ${mPart}`;
}

export function formatFriendlyDate(isoDate?: string | null): string {
  let date: Date;
  if (isoDate && /^\d{4}-\d{2}-\d{2}$/.test(isoDate)) {
    const [year, month, day] = isoDate.split("-").map(Number);
    date = new Date(year, month - 1, day);
  } else {
    date = new Date();
  }

  return new Intl.DateTimeFormat("en-US", {
    weekday: "long",
    month: "long",
    day: "numeric",
  }).format(date);
}

export function computeRelativeBarPercent(activeSecs: number, maxSecs: number): number {
  if (activeSecs <= 0 || maxSecs <= 0) return 0;
  const raw = (activeSecs / maxSecs) * 100;
  return Math.max(4, Math.min(100, Number(raw.toFixed(1))));
}
