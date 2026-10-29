import type { Recurrence, Tag, TaskList } from "../types";

const weekdayNames = [
  ["sunday", "dimanche"],
  ["monday", "lundi"],
  ["tuesday", "mardi"],
  ["wednesday", "mercredi"],
  ["thursday", "jeudi"],
  ["friday", "vendredi"],
  ["saturday", "samedi"],
];
const weekdayPattern = weekdayNames.flat().join("|");
const dayIndex = (value: string) =>
  weekdayNames.findIndex((names) => names.includes(value.replace(/s$/, "")));
const localDay = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
type Span = { start: number; end: number };
function span(match: RegExpMatchArray): Span {
  return { start: match.index!, end: match.index! + match[0].length };
}
function calendarDate(year: number, month: number, day: number) {
  const date = new Date(year, month - 1, day, 12);
  return year >= 1900 &&
    year <= 9999 &&
    date.getFullYear() === year &&
    date.getMonth() === month - 1 &&
    date.getDate() === day
    ? date
    : null;
}
function addCalendar(
  date: Date,
  amount: number,
  unit: "day" | "week" | "month" | "year",
) {
  const result = new Date(date);
  if (unit === "day" || unit === "week")
    result.setDate(result.getDate() + amount * (unit === "week" ? 7 : 1));
  else {
    const day = result.getDate();
    result.setDate(1);
    if (unit === "month") result.setMonth(result.getMonth() + amount);
    else result.setFullYear(result.getFullYear() + amount);
    const end = new Date(
      result.getFullYear(),
      result.getMonth() + 1,
      0,
    ).getDate();
    result.setDate(Math.min(day, end));
  }
  return result;
}

/** A conservative, local-calendar parser. Unrecognised/invalid phrases remain text. */
export function parseTaskInput(
  value: string,
  settings: Record<string, unknown>,
  lists: TaskList[],
  tags: Tag[],
  referenceDate = new Date(),
): { title: string } & Record<string, unknown> {
  let title = value.trim();
  const patch: Record<string, unknown> = {};
  const tagIds = new Set<string>();
  title = title.replace(
    /(?:^|\s)!([0-3]|high|medium|low|none)\b/gi,
    (_, p: string) => {
      patch.priority = (
        {
          "1": 3,
          "2": 2,
          "3": 1,
          "0": 0,
          high: 3,
          medium: 2,
          low: 1,
          none: 0,
        } as Record<string, number>
      )[p.toLowerCase()];
      return " ";
    },
  );
  for (const list of [...lists]
    .filter((list) => !list.archived_at)
    .sort((a, b) => b.name.length - a.name.length)) {
    const pattern = new RegExp(
      `(?:^|\\s)[~^]${escapeRegExp(list.name)}(?=\\s|$)`,
      "i",
    );
    if (pattern.test(title)) {
      title = title.replace(pattern, " ");
      patch.list_id = list.id;
      break;
    }
  }
  title = title.replace(/(?:^|\s)#([^\s#]+)/g, (full, name: string) => {
    const tag = tags.find(
      (t) => t.name.toLocaleLowerCase() === name.toLocaleLowerCase(),
    );
    if (tag) tagIds.add(tag.id);
    return tag && settings.tag_recognition !== "keep" ? " " : full;
  });
  if (tagIds.size) patch.tag_ids = [...tagIds];
  if (
    settings.date_recognition !== "off" &&
    !Number.isNaN(referenceDate.getTime())
  ) {
    const lower = title.toLocaleLowerCase().replace(/’/g, "'");
    const ranges: Span[] = [];
    let date: Date | null = null;
    let time: string | null = null;
    let recurrence: Recurrence | undefined;
    let explicitRepeatWeekdays = false;
    const base = new Date(referenceDate);
    base.setHours(12, 0, 0, 0);
    const rule = (
      frequency: Recurrence["frequency"],
      interval = 1,
      weekdays: number[] = [],
    ): Recurrence => ({
      frequency,
      interval,
      weekdays,
      end_date: null,
      count: null,
      mode: "due_date",
    });
    // Repeating phrases are consumed before weekdays, so "every Monday" is
    // never reduced to an ordinary single Monday task.
    const repeatDay = lower.match(
      new RegExp(
        `(?:^|\\s)(?:every\\s+|tous\\s+les\\s+)(${weekdayPattern})s?(?=\\s|$|[,;])`,
        "i",
      ),
    );
    const repeatUnits = lower.match(
      /(?:^|\s)(?:every\s+|tous\s+les\s+|toutes\s+les\s+)(?:(\d{1,3})\s+)?(days?|jours?|weeks?|semaines?|months?|mois|years?|ans?|années?)(?=\s|$|[,;])/i,
    );
    const repeatSimple = lower.match(
      /(?:^|\s)(daily|weekly|monthly|yearly|quotidiennement|chaque jour|chaque semaine|chaque mois|chaque année|every weekday|tous les jours ouvrés)(?=\s|$|[,;])/i,
    );
    if (repeatDay) {
      const index = dayIndex(repeatDay[1]!);
      recurrence = rule("weekly", 1, [index || 7]);
      explicitRepeatWeekdays = true;
      date = addCalendar(base, (index - base.getDay() + 7) % 7, "day");
      ranges.push(span(repeatDay));
    } else if (repeatUnits && !repeatSimple?.[1]?.includes("ouvrés")) {
      const interval = Number(repeatUnits[1] || 1);
      if (interval >= 1 && interval <= 365) {
        const unit = repeatUnits[2]!;
        const frequency = /^(day|jour)/.test(unit)
          ? "daily"
          : /^(week|semaine)/.test(unit)
            ? "weekly"
            : /^(month|mois)/.test(unit)
              ? "monthly"
              : "yearly";
        recurrence = rule(
          frequency,
          interval,
          frequency === "weekly" ? [base.getDay() || 7] : [],
        );
        date = base;
        ranges.push(span(repeatUnits));
      }
    } else if (repeatSimple) {
      const unit = repeatSimple[1]!;
      const workdays = /weekday|ouvrés/.test(unit);
      explicitRepeatWeekdays = workdays;
      const frequency =
        /daily|quotidiennement|jour/.test(unit) && !workdays
          ? "daily"
          : /weekly|semaine/.test(unit) || workdays
            ? "weekly"
            : /monthly|mois/.test(unit)
              ? "monthly"
              : "yearly";
      recurrence = rule(
        frequency,
        1,
        workdays
          ? [1, 2, 3, 4, 5]
          : frequency === "weekly"
            ? [base.getDay() || 7]
            : [],
      );
      date =
        workdays && [0, 6].includes(base.getDay())
          ? addCalendar(base, base.getDay() === 6 ? 2 : 1, "day")
          : base;
      ranges.push(span(repeatSimple));
    }
    const available = (match: RegExpMatchArray | null) =>
      match &&
      !ranges.some(
        (range) =>
          match.index! < range.end &&
          match.index! + match[0].length > range.start,
      )
        ? match
        : null;
    // An explicit date can anchor a recurrence ("every week 2026-11-02").
    const iso = available(
      lower.match(/(?:^|\s)\*?(\d{4})-(\d{2})-(\d{2})(?=\s|$|[,;])/),
    );
    const numeric = available(
      lower.match(/(?:^|\s)(\d{1,2})\/(\d{1,2})(?:\/(\d{4}))?(?=\s|$|[,;])/),
    );
    const relative = available(
      lower.match(
        /(?:^|\s)(?:in|dans)\s+(\d{1,4})\s+(minutes?|mins?|hours?|heures?|days?|jours?|weeks?|semaines?|months?|mois|years?|ans?|années?)(?=\s|$|[,;])/,
      ),
    );
    const named = available(
      lower.match(
        /(?:^|\s)\*?(day after tomorrow|après-demain|apres-demain|today|tomorrow|aujourd'hui|demain|next week|la semaine prochaine|semaine prochaine|next month|le mois prochain|mois prochain|next year|l'année prochaine)(?=\s|$|[,;])/,
      ),
    );
    const weekday = available(
      lower.match(
        new RegExp(
          `(?:^|\\s)(?:(next|this)\\s+)?(${weekdayPattern})(?:\\s+(prochain|prochaine))?(?=\\s|$|[,;])`,
          "i",
        ),
      ),
    );
    let dateMatch: RegExpMatchArray | null = null;
    if (iso) {
      const parsed = calendarDate(
        Number(iso[1]),
        Number(iso[2]),
        Number(iso[3]),
      );
      if (parsed) {
        date = parsed;
        dateMatch = iso;
      }
    } else if (numeric) {
      // Slash dates consistently use day/month; never silently switch locale.
      let year = Number(numeric[3] || base.getFullYear());
      let parsed = calendarDate(year, Number(numeric[2]), Number(numeric[1]));
      if (parsed && !numeric[3] && localDay(parsed) < localDay(base))
        parsed = calendarDate(++year, Number(numeric[2]), Number(numeric[1]));
      if (parsed) {
        date = parsed;
        dateMatch = numeric;
      }
    } else if (relative) {
      const amount = Number(relative[1]);
      const unit = relative[2]!;
      if (amount > 0 && amount <= 3650) {
        if (/^(minute|min|hour|heure)/.test(unit)) {
          date = new Date(
            referenceDate.getTime() +
              amount * (/^(hour|heure)/.test(unit) ? 60 : 1) * 60_000,
          );
          time = `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
        } else
          date = addCalendar(
            base,
            amount,
            /^(week|semaine)/.test(unit)
              ? "week"
              : /^(month|mois)/.test(unit)
                ? "month"
                : /^(year|an)/.test(unit)
                  ? "year"
                  : "day",
          );
        dateMatch = relative;
      }
    } else if (named) {
      const token = named[1]!;
      date = /after|après|apres/.test(token)
        ? addCalendar(base, 2, "day")
        : /tomorrow|demain/.test(token)
          ? addCalendar(base, 1, "day")
          : /week|semaine/.test(token)
            ? addCalendar(base, 1, "week")
            : /month|mois/.test(token)
              ? addCalendar(base, 1, "month")
              : /year|année/.test(token)
                ? addCalendar(base, 1, "year")
                : base;
      dateMatch = named;
    } else if (weekday) {
      const offset = (dayIndex(weekday[2]!) - base.getDay() + 7) % 7;
      date = addCalendar(
        base,
        offset || (weekday[1] === "this" ? 0 : 7),
        "day",
      );
      dateMatch = weekday;
    }
    if (dateMatch) ranges.push(span(dateMatch));
    const timeMatch = [
      ...lower.matchAll(
        /(?:^|\s)(?:(at|à)\s+)?(\d{1,2})(?:(:|h)(\d{2})?)?(?:\s*(am|pm))?(?=\s|$|[,;])/gi,
      ),
    ].find((match) => available(match) && (match[1] || match[3] || match[5]));
    if (timeMatch) {
      let hour = Number(timeMatch[2]);
      const minute = Number(timeMatch[4] || 0);
      const meridiem = timeMatch[5];
      if (
        minute < 60 &&
        (meridiem ? hour >= 1 && hour <= 12 : hour <= 23) &&
        !(timeMatch[3] === ":" && !timeMatch[4])
      ) {
        if (meridiem) hour = (hour % 12) + (meridiem === "pm" ? 12 : 0);
        time = `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
        if (!date) {
          date = base;
          if (
            hour * 60 + minute <
            referenceDate.getHours() * 60 + referenceDate.getMinutes()
          )
            date = addCalendar(base, 1, "day");
        }
        ranges.push(span(timeMatch));
      }
    } else {
      const midday = lower.match(
        /(?:^|\s)(?:at\s+|à\s+)?(noon|midnight|midi|minuit)(?=\s|$|[,;])/i,
      );
      if (midday) {
        time = /noon|midi$/.test(midday[1]!) ? "12:00" : "00:00";
        date ||= base;
        ranges.push(span(midday));
      }
    }
    if (date && date.getFullYear() <= 9999) {
      patch.due_date = localDay(date);
      if (time) patch.due_time = time;
      if (recurrence?.frequency === "weekly" && !explicitRepeatWeekdays)
        recurrence.weekdays = [date.getDay() || 7];
      if (recurrence)
        patch.recurrence = { ...recurrence, anchor_date: localDay(date) };
      if (settings.date_recognition !== "keep")
        for (const range of ranges.sort((a, b) => b.start - a.start))
          title = title.slice(0, range.start) + " " + title.slice(range.end);
    }
  }
  return { title: title.replace(/\s+/g, " ").trim() || value.trim(), ...patch };
}
function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Ignore empty clipboard rows, preserve user text (including bullet markers). */
export function splitTaskLines(text: string): string[] {
  return text
    .split(/\r\n|\n|\r/)
    .map((line) => line.trim())
    .filter(Boolean);
}
export const defaultHotkeys = {
  quick_add: "mod+n",
  search: "mod+k",
  settings: "mod+,",
  shortcuts: "?",
  complete_task: "mod+enter",
  delete_task: "mod+backspace",
  add_subtask: "shift+enter",
  toggle_all_groups: "",
  toggle_all_subtasks: "",
  print_task: "mod+p",
  set_date: "mod+d",
  pin_task: "",
  clear_date: "",
  set_today: "",
  set_tomorrow: "",
  set_next_week: "",
  priority_none: "",
  priority_low: "",
  priority_medium: "",
  priority_high: "",
  list_view: "v l",
  kanban_view: "v k",
  today: "g t",
  tomorrow: "g r",
  upcoming: "g n",
  inbox: "g i",
  all: "g a",
  completed: "g c",
  wont_do: "g w",
  trash: "g g",
} as const;
export type HotkeyAction = keyof typeof defaultHotkeys;
export function keyChord(
  event: KeyboardEvent,
  options: { physical?: boolean } = {},
) {
  let key = event.key.toLowerCase();
  const physicalKey = /^(?:Key([A-Z])|Digit([0-9]))$/.exec(event.code);
  // Option can emit ©, ∆, or a dead key instead of the shortcut's letter.
  // Keep ordinary layout-aware letters (for example AZERTY), and only use
  // the physical key when the modified character cannot identify that key.
  const usePhysicalKey =
    physicalKey &&
    (options.physical ||
      key === "dead" ||
      (event.altKey && !/^[a-z0-9]$/.test(key)));
  if (usePhysicalKey) key = (physicalKey[1] || physicalKey[2])!.toLowerCase();
  const parts: string[] = [];
  const isMac = /Mac/.test(navigator.platform);
  if (event.metaKey || (event.ctrlKey && !isMac)) parts.push("mod");
  if (event.ctrlKey && (isMac || event.metaKey)) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (
    event.shiftKey &&
    (options.physical ||
      key.length !== 1 ||
      /^[a-z]$/.test(key) ||
      usePhysicalKey)
  )
    parts.push("shift");
  if (key === " ") key = "space";
  if (["meta", "control", "shift", "alt"].includes(key)) return "";
  parts.push(key);
  return parts.join("+");
}
