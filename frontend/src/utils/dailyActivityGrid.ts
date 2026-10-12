// Grid math for the new-thread activity heatmap: one column per week, seven
// rows per weekday. All calendar math stays in UTC so a browser timezone can
// never shift a server-side `YYYY-MM-DD` bucket into a neighbouring cell.

export type DailyActivityPoint = {
  date?: unknown;
  value?: unknown;
};

export type ActivityCell = {
  date: string;
  day: number;
  month: number;
  value: number;
  level: number;
  future: boolean;
  isToday: boolean;
};

export type ActivityColumn = {
  weekStart: string;
  cells: ActivityCell[];
};

export type ActivityMonthLabel = {
  month: number;
  column: number;
};

export type ActivityGrid = {
  columns: ActivityColumn[];
  monthLabels: ActivityMonthLabel[];
  activeDays: number;
  total: number;
  peak: number;
};

const DAY_MS = 86_400_000;
const WEEK_DAYS = 7;
const MIN_MONTH_LABEL_GAP = 4;

export const ACTIVITY_WEEK_COLUMNS = 53;

const DAY_KEY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

export const formatDayKey = (timestamp: number): string =>
  new Date(timestamp).toISOString().slice(0, 10);

export const parseDayKey = (value: unknown): number | null => {
  const match = DAY_KEY_PATTERN.exec(String(value ?? '').trim());
  if (!match) {
    return null;
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) {
    return null;
  }
  const timestamp = Date.UTC(year, month - 1, day);
  // Date.UTC rolls impossible dates over into the next month; a round trip
  // keeps them out of the grid instead of silently moving activity.
  if (Number.isNaN(timestamp) || formatDayKey(timestamp) !== match[0]) {
    return null;
  }
  return timestamp;
};

const toSafeValue = (value: unknown): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.trunc(parsed) : 0;
};

// Steps relative to the rendered peak keep the ramp stable for both sparse
// session counts and large credit totals.
export const resolveActivityLevel = (value: number, peak: number): number => {
  if (value <= 0 || peak <= 0) {
    return 0;
  }
  if (value >= peak * 0.75) {
    return 4;
  }
  if (value >= peak * 0.5) {
    return 3;
  }
  if (value >= peak * 0.25) {
    return 2;
  }
  return 1;
};

const buildMonthLabels = (columns: ActivityColumn[]): ActivityMonthLabel[] => {
  const labels: ActivityMonthLabel[] = [];
  columns.forEach((column, index) => {
    const first = column.cells[0];
    const last = column.cells[column.cells.length - 1];
    // A month is only named on a week that sits fully inside it, so a label
    // never claims a month the column barely touches.
    if (!first || !last || first.month !== last.month) {
      return;
    }
    const previous = labels[labels.length - 1];
    if (previous && (previous.month === first.month || index - previous.column < MIN_MONTH_LABEL_GAP)) {
      return;
    }
    // Keep labels clear of the right edge: a month starting in the last few
    // weeks is still visible through its cells, just not named.
    if (index > columns.length - 4) {
      return;
    }
    labels.push({ month: first.month, column: index });
  });
  return labels;
};

export const buildActivityGrid = (
  points: DailyActivityPoint[] | null | undefined,
  todayKey: string,
  weeks: number = ACTIVITY_WEEK_COLUMNS
): ActivityGrid => {
  const empty: ActivityGrid = {
    columns: [],
    monthLabels: [],
    activeDays: 0,
    total: 0,
    peak: 0
  };
  const today = parseDayKey(todayKey);
  if (today === null || !Number.isFinite(weeks) || weeks <= 0) {
    return empty;
  }
  const values = new Map<string, number>();
  for (const point of Array.isArray(points) ? points : []) {
    const date = String(point?.date ?? '').trim();
    if (parseDayKey(date) === null) {
      continue;
    }
    const value = toSafeValue(point?.value);
    if (value <= 0) {
      continue;
    }
    values.set(date, (values.get(date) ?? 0) + value);
  }

  // The grid ends on the Saturday of the current week so every column keeps
  // the same weekday row; days after today become future placeholders.
  const weekdayOfToday = new Date(today).getUTCDay();
  const endOfTodayWeek = today + (WEEK_DAYS - 1 - weekdayOfToday) * DAY_MS;
  const start = endOfTodayWeek - (weeks * WEEK_DAYS - 1) * DAY_MS;

  // Stats and the colour ramp only cover what is actually rendered, so a
  // wider server window can never skew the levels.
  let total = 0;
  let peak = 0;
  let activeDays = 0;
  for (const [date, value] of values) {
    const timestamp = parseDayKey(date);
    if (timestamp === null || timestamp < start || timestamp > endOfTodayWeek) {
      continue;
    }
    total += value;
    activeDays += 1;
    if (value > peak) {
      peak = value;
    }
  }

  const columns: ActivityColumn[] = [];
  for (let week = 0; week < weeks; week += 1) {
    const weekStart = start + week * WEEK_DAYS * DAY_MS;
    const cells: ActivityCell[] = [];
    for (let offset = 0; offset < WEEK_DAYS; offset += 1) {
      const timestamp = weekStart + offset * DAY_MS;
      const date = new Date(timestamp);
      const key = formatDayKey(timestamp);
      const value = values.get(key) ?? 0;
      cells.push({
        date: key,
        day: date.getUTCDate(),
        month: date.getUTCMonth(),
        value,
        level: resolveActivityLevel(value, peak),
        future: timestamp > today,
        isToday: timestamp === today
      });
    }
    columns.push({ weekStart: formatDayKey(weekStart), cells });
  }

  return { columns, monthLabels: buildMonthLabels(columns), activeDays, total, peak };
};

export const formatActivityCount = (value: number): string => {
  if (!Number.isFinite(value) || value <= 0) {
    return '0';
  }
  if (value < 1000) {
    return String(value);
  }
  if (value < 1_000_000) {
    return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}k`;
  }
  return `${(value / 1_000_000).toFixed(1)}M`;
};
