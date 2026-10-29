<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import {
  PhAlarm,
  PhCaretDown,
  PhCaretLeft,
  PhCaretRight,
  PhCircle,
  PhClock,
  PhCheck,
  PhMoon,
  PhRepeat,
  PhRepeatOnce,
  PhSun,
  PhSunHorizon,
  PhX,
} from "@phosphor-icons/vue";
import type { Task, Recurrence } from "../types";
import { dispatch } from "../adapter";
import DatePickerSelect from "./DatePickerSelect.vue";
import { registerEscapeLayer } from "../lib/escape-layers";

type DatedTask = Task & {
  start_date?: string | null;
  start_time?: string | null;
  duration_minutes?: number | null;
  timezone?: string;
  recurrence?: Recurrence | null;
  reminders?: string[];
};
const props = defineProps<{
  task: DatedTask;
  settings: Record<string, unknown>;
  newTask?: boolean;
}>();
const emit = defineEmits<{
  save: [patch: Record<string, unknown>];
  close: [];
}>();
const root = ref<HTMLElement>();
let unregisterEscapeLayer: (() => void) | undefined;
type DateSelectHandle = { closeOpenMenu: () => boolean };
const dateSelects = new Map<string, DateSelectHandle>();
function rememberDateSelect(name: string, instance: unknown) {
  if (instance) dateSelects.set(name, instance as DateSelectHandle);
  else dateSelects.delete(name);
}
function escapeFromOutside(event: KeyboardEvent) {
  if (
    event.key !== "Escape" ||
    event.isComposing ||
    event.defaultPrevented ||
    !root.value ||
    (event.target instanceof Node && root.value.contains(event.target))
  )
    return;
  event.preventDefault();
  event.stopImmediatePropagation();
  for (const select of [...dateSelects.values()].reverse()) {
    if (select.closeOpenMenu()) return;
  }
  escape(event);
}

const localZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
const zone = ref(props.task.timezone || localZone);
function dateString(value: Date) {
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
}
function parseDate(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day, 12);
}
function addDays(value: string, amount: number) {
  const result = parseDate(value);
  result.setDate(result.getDate() + amount);
  return dateString(result);
}
const today = dateString(new Date());
const date = ref(props.task.due_date || "");
const time = ref(props.task.due_time || "");
const startDate = ref(props.task.start_date || props.task.due_date || today);
const startTime = ref(props.task.start_time || "");
const tab = ref<"date" | "duration">(
  props.task.start_date ? "duration" : "date",
);
const endpoint = ref<"start" | "end">("end");
const month = ref(parseDate(date.value || today));
const expanded = ref<"time" | "reminder" | "repeat" | "end" | null>(null);
const error = ref("");
const submenu = ref<HTMLElement>();
const submenuStyle = ref<Record<string, string>>({});
const timeInput = ref<HTMLInputElement>();
const timeList = ref<HTMLElement>();
const customReminderOpen = ref(false);
const times = Array.from(
  { length: 48 },
  (_, index) =>
    `${String(Math.floor(index / 2)).padStart(2, "0")}:${index % 2 ? "30" : "00"}`,
);
const initialReminders = [
  ...(props.task.reminders ||
    (props.task.reminder_at ? [props.task.reminder_at] : [])),
];
const remindersChanged = ref(false);
const selectedOffsets = ref<number[]>([]);
const customReminders = ref<{ date: string; time: string }[]>([]);
const customDate = ref(date.value || today);
const customTime = ref("09:00");
const reminderPresets = computed(() =>
  time.value
    ? [
        { minutes: 0, label: "On time", suffix: "" },
        { minutes: 5, label: "5 minutes early", suffix: "" },
        { minutes: 30, label: "30 minutes early", suffix: "" },
        { minutes: 60, label: "1 hour early", suffix: "" },
        { minutes: 1440, label: "1 day early", suffix: "" },
      ]
    : [
        { minutes: 0, label: "On the day", suffix: "(09:00)" },
        { minutes: 1440, label: "1 day early", suffix: "(09:00)" },
        { minutes: 2880, label: "2 days early", suffix: "(09:00)" },
        { minutes: 4320, label: "3 days early", suffix: "(09:00)" },
        { minutes: 10080, label: "1 week early", suffix: "(09:00)" },
      ],
);
const reminderUnit = ref<"minute" | "hour" | "day" | "week">(
  time.value ? "minute" : "day",
);
const reminderAmount = ref(1);
const reminderUnits = computed(() =>
  time.value
    ? (["minute", "hour", "day", "week"] as const)
    : (["day", "week"] as const),
);
let reminderSnapshot: {
  offsets: number[];
  custom: { date: string; time: string }[];
  changed: boolean;
} | null = null;
function captureReminders() {
  reminderSnapshot = {
    offsets: [...selectedOffsets.value],
    custom: customReminders.value.map((item) => ({ ...item })),
    changed: remindersChanged.value,
  };
}
function cancelReminders() {
  if (reminderSnapshot) {
    selectedOffsets.value = [...reminderSnapshot.offsets];
    customReminders.value = reminderSnapshot.custom.map((item) => ({
      ...item,
    }));
    remindersChanged.value = reminderSnapshot.changed;
  }
  closeSubmenu();
}
function confirmReminders() {
  reminderSnapshot = null;
  closeSubmenu();
}
const customReminderPreview = computed(() => {
  try {
    if (
      !Number.isInteger(reminderAmount.value) ||
      reminderAmount.value < 0 ||
      reminderAmount.value > 10000
    )
      return null;
    const factor = { minute: 1, hour: 60, day: 1440, week: 10080 }[
      reminderUnit.value
    ];
    const instant = !time.value
      ? zonedInstant(
          addDays(date.value || today, (-reminderAmount.value * factor) / 1440),
          customTime.value || "09:00",
          zone.value,
        )
      : zonedInstant(date.value || today, time.value, zone.value) -
        reminderAmount.value * factor * 60000;
    return { instant, ...localParts(new Date(instant), zone.value) };
  } catch {
    return null;
  }
});
function openCustomReminder() {
  reminderUnit.value = time.value ? "minute" : "day";
  reminderAmount.value = time.value ? 30 : 1;
  customReminderOpen.value = true;
  nextTick(() =>
    submenu.value
      ?.querySelector<HTMLInputElement>('[aria-label="Reminder amount"]')
      ?.focus(),
  );
}
function confirmCustomReminder() {
  const value = customReminderPreview.value;
  if (!value) {
    error.value = "Choose a valid reminder.";
    return;
  }
  if (
    !customReminders.value.some(
      (item) => item.date === value.date && item.time === value.time,
    )
  )
    customReminders.value.push({ date: value.date, time: value.time });
  remindersChanged.value = true;
  error.value = "";
  confirmReminders();
}
function customReminderLabel(reminder: { date: string; time: string }) {
  try {
    const minutes = Math.round(
      (zonedInstant(date.value || today, time.value || "09:00", zone.value) -
        zonedInstant(reminder.date, reminder.time, zone.value)) /
        60000,
    );
    if (minutes === 0)
      return time.value
        ? "On time"
        : `On the day (${prettyTime(reminder.time)})`;
    if (minutes > 0 && time.value) {
      if (minutes % 1440 === 0)
        return `${minutes / 1440} ${minutes === 1440 ? "day" : "days"} early`;
      if (minutes % 60 === 0)
        return `${minutes / 60} ${minutes === 60 ? "hour" : "hours"} early`;
      return `${minutes} ${minutes === 1 ? "minute" : "minutes"} early`;
    }
    return `${reminder.date} ${prettyTime(reminder.time)}`;
  } catch {
    return `${reminder.date} ${reminder.time}`;
  }
}
const frequency = ref<Recurrence["frequency"]>(
  props.task.recurrence?.frequency || "daily",
);
const interval = ref(props.task.recurrence?.interval || 1);
const weekdays = ref<number[]>([...(props.task.recurrence?.weekdays || [])]);
const repeatEnabled = ref(!!props.task.recurrence);
const endKind = ref<"never" | "date" | "count">(
  props.task.recurrence?.end_date
    ? "date"
    : props.task.recurrence?.count
      ? "count"
      : "never",
);
const endDate = ref(props.task.recurrence?.end_date || "");
const repeatCount = ref(props.task.recurrence?.count || 2);
const endPanel = ref<"menu" | "date" | "count">("menu");
let endSnapshot = {
  kind: endKind.value,
  date: endDate.value,
  count: repeatCount.value,
};
const endSummary = computed(() =>
  endKind.value === "never"
    ? "Endlessly"
    : endKind.value === "date"
      ? `Until ${endDate.value}`
      : `Repeat ${repeatCount.value} Times`,
);
const repeatMode = ref<Recurrence["mode"]>(
  props.task.recurrence?.mode || "due_date",
);
const customRepeat = ref(false);
type DetailedRecurrence = {
  month_days?: number[];
  ordinal_weekday?: { ordinal: number; weekday: number } | null;
  workday?: "first" | "last" | null;
  year_month?: number | null;
  skip_weekends?: boolean;
};
type RecurrenceDetailsSnapshot = {
  monthlyMode: "each" | "ordinal" | "workday";
  monthDays: number[];
  ordinal: number;
  ordinalDay: number;
  monthWorkday: "first" | "last";
  yearMonth: number;
  skipWeekends: boolean;
  patternEnabled: boolean;
  ordinalDefaults: { monthly: boolean; yearly: boolean };
};
const initialRecurrenceDetails = (props.task.recurrence ||
  {}) as DetailedRecurrence;
const recurrenceAnchor = computed(() => parseDate(date.value || today));
const monthlyMode = ref<"each" | "ordinal" | "workday">(
  initialRecurrenceDetails.workday
    ? "workday"
    : initialRecurrenceDetails.ordinal_weekday
      ? "ordinal"
      : "each",
);
const monthDays = ref<number[]>(
  initialRecurrenceDetails.month_days?.length
    ? [...initialRecurrenceDetails.month_days]
    : [recurrenceAnchor.value.getDate()],
);
const ordinal = ref(initialRecurrenceDetails.ordinal_weekday?.ordinal || 1);
const ordinalDay = ref(
  initialRecurrenceDetails.ordinal_weekday?.weekday ||
    recurrenceAnchor.value.getDay() ||
    7,
);
const monthWorkday = ref<"first" | "last">(
  initialRecurrenceDetails.workday || "first",
);
const yearMonth = ref(
  initialRecurrenceDetails.year_month || recurrenceAnchor.value.getMonth() + 1,
);
const skipWeekends = ref(!!initialRecurrenceDetails.skip_weekends);
const recurrencePatternEnabled = ref(
  !!(
    initialRecurrenceDetails.month_days?.length ||
    initialRecurrenceDetails.ordinal_weekday ||
    initialRecurrenceDetails.workday ||
    initialRecurrenceDetails.year_month ||
    initialRecurrenceDetails.skip_weekends
  ),
);
const ordinalDefaults = ref({
  monthly: !!initialRecurrenceDetails.ordinal_weekday,
  yearly: !!initialRecurrenceDetails.ordinal_weekday,
});
const recurrenceMonths = Array.from({ length: 12 }, (_, index) =>
  new Date(2000, index, 1).toLocaleDateString("en", { month: "long" }),
);
const recurrenceOrdinals = [
  { value: 1, label: "First" },
  { value: 2, label: "Second" },
  { value: 3, label: "Third" },
  { value: 4, label: "Fourth" },
  { value: 5, label: "Fifth" },
  { value: -1, label: "Last" },
];
const recurrenceWeekdays = [1, 2, 3, 4, 5, 6, 7];
const selectedRepeatWeekdays = computed(() =>
  weekdays.value.length
    ? weekdays.value
    : [recurrenceAnchor.value.getDay() || 7],
);
const repeatMonthDayCount = computed(() =>
  frequency.value === "yearly"
    ? new Date(2000, yearMonth.value, 0).getDate()
    : 31,
);
const repeatCalendarDays = computed(() =>
  Array.from({ length: repeatMonthDayCount.value }, (_, index) => index + 1),
);

function changeCustomFrequency(next: Recurrence["frequency"]) {
  frequency.value = next;
  if (next === "weekly" && !weekdays.value.length)
    weekdays.value = [recurrenceAnchor.value.getDay() || 7];
  if (next === "yearly") {
    if (monthlyMode.value === "workday") monthlyMode.value = "each";
    const candidate =
      monthDays.value.find((day) => day > 0) ||
      recurrenceAnchor.value.getDate();
    monthDays.value = [Math.min(candidate, repeatMonthDayCount.value)];
  }
  if (
    (next === "monthly" || next === "yearly") &&
    monthlyMode.value === "ordinal"
  )
    initializeOrdinalMode(next);
}
function initializeOrdinalMode(unit: "monthly" | "yearly") {
  if (ordinalDefaults.value[unit]) return;
  ordinal.value =
    unit === "yearly"
      ? Math.floor((recurrenceAnchor.value.getDate() - 1) / 7) + 1
      : 1;
  ordinalDay.value = recurrenceAnchor.value.getDay() || 7;
  ordinalDefaults.value[unit] = true;
}
function chooseMonthlyMode(mode: "each" | "ordinal" | "workday") {
  monthlyMode.value = mode;
  if (mode === "ordinal")
    initializeOrdinalMode(frequency.value === "yearly" ? "yearly" : "monthly");
  if (mode === "each" && !monthDays.value.length)
    monthDays.value = [recurrenceAnchor.value.getDate()];
}
function toggleCustomWeekday(day: number) {
  const current = [...selectedRepeatWeekdays.value];
  if (current.includes(day)) {
    if (current.length > 1)
      weekdays.value = current.filter((value) => value !== day);
  } else weekdays.value = [...current, day].sort((a, b) => a - b);
}
function selectMonthDay(day: number) {
  if (frequency.value === "yearly") {
    monthDays.value = [day];
    return;
  }
  if (monthDays.value.includes(day)) {
    if (monthDays.value.length > 1)
      monthDays.value = monthDays.value.filter((value) => value !== day);
  } else
    monthDays.value = [...monthDays.value, day].sort(
      (a, b) => (a === -1 ? 32 : a) - (b === -1 ? 32 : b),
    );
}
function changeYearMonth(monthNumber: number) {
  yearMonth.value = ((monthNumber + 11) % 12) + 1;
  if (frequency.value === "yearly" && monthlyMode.value === "each") {
    const candidate =
      monthDays.value.find((day) => day > 0) ||
      recurrenceAnchor.value.getDate();
    monthDays.value = [Math.min(candidate, repeatMonthDayCount.value)];
  }
}
function commitRecurrenceDetails() {
  recurrencePatternEnabled.value = true;
  if (frequency.value === "weekly" && !weekdays.value.length)
    weekdays.value = [recurrenceAnchor.value.getDay() || 7];
  if (frequency.value === "yearly" && monthlyMode.value === "each")
    changeYearMonth(yearMonth.value);
}
function recurrenceDetails(): Required<DetailedRecurrence> {
  const empty: Required<DetailedRecurrence> = {
    month_days: [],
    ordinal_weekday: null,
    workday: null,
    year_month: null,
    skip_weekends: false,
  };
  if (
    !recurrencePatternEnabled.value ||
    (frequency.value !== "monthly" && frequency.value !== "yearly")
  )
    return empty;
  const yearly = frequency.value === "yearly";
  if (monthlyMode.value === "ordinal")
    return {
      ...empty,
      year_month: yearly ? yearMonth.value : null,
      ordinal_weekday: { ordinal: ordinal.value, weekday: ordinalDay.value },
    };
  if (!yearly && monthlyMode.value === "workday")
    return { ...empty, workday: monthWorkday.value };
  return {
    ...empty,
    month_days: yearly
      ? [
          monthDays.value.find((day) => day > 0) ||
            recurrenceAnchor.value.getDate(),
        ]
      : [...monthDays.value],
    year_month: yearly ? yearMonth.value : null,
    skip_weekends: !yearly && skipWeekends.value,
  };
}
function recurrencePatternSummary(): string | null {
  if (
    !recurrencePatternEnabled.value ||
    (frequency.value !== "monthly" && frequency.value !== "yearly")
  )
    return null;
  const yearly = frequency.value === "yearly";
  const cadence =
    interval.value === 1
      ? yearly
        ? "Yearly"
        : "Monthly"
      : `Every ${interval.value} ${yearly ? "years" : "months"}`;
  const shortMonth = recurrenceMonths[yearMonth.value - 1].slice(0, 3);
  const ordinalText = (value: number) => {
    if (value === -1) return "last";
    const suffix =
      value % 100 >= 11 && value % 100 <= 13
        ? "th"
        : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[
            value % 10
          ] || "th";
    return `${value}${suffix}`;
  };
  if (monthlyMode.value === "ordinal")
    return `${cadence} on the ${ordinalText(ordinal.value)} ${dayNames[ordinalDay.value % 7]}${yearly ? ` in ${shortMonth}` : ""}`;
  if (!yearly && monthlyMode.value === "workday")
    return `${cadence} on the ${monthWorkday.value} workday`;
  if (yearly)
    return `${cadence} on ${shortMonth} ${monthDays.value.find((day) => day > 0) || recurrenceAnchor.value.getDate()}`;
  const days = new Intl.ListFormat("en", {
    style: "long",
    type: "conjunction",
  }).format(
    monthDays.value.map((day) => (day === -1 ? "the last day" : String(day))),
  );
  const monthlyCadence =
    interval.value === 1 ? "Every month" : `Every ${interval.value} months`;
  return `${monthlyCadence} on day ${days}${skipWeekends.value ? " · Skip Weekends" : ""}`;
}
function captureRecurrenceDetails(): RecurrenceDetailsSnapshot {
  return {
    monthlyMode: monthlyMode.value,
    monthDays: [...monthDays.value],
    ordinal: ordinal.value,
    ordinalDay: ordinalDay.value,
    monthWorkday: monthWorkday.value,
    yearMonth: yearMonth.value,
    skipWeekends: skipWeekends.value,
    patternEnabled: recurrencePatternEnabled.value,
    ordinalDefaults: { ...ordinalDefaults.value },
  };
}
function restoreRecurrenceDetails(snapshot: RecurrenceDetailsSnapshot) {
  monthlyMode.value = snapshot.monthlyMode;
  monthDays.value = [...snapshot.monthDays];
  ordinal.value = snapshot.ordinal;
  ordinalDay.value = snapshot.ordinalDay;
  monthWorkday.value = snapshot.monthWorkday;
  yearMonth.value = snapshot.yearMonth;
  skipWeekends.value = snapshot.skipWeekends;
  recurrencePatternEnabled.value = snapshot.patternEnabled;
  ordinalDefaults.value = { ...snapshot.ordinalDefaults };
}
function resetRecurrenceDetails() {
  monthlyMode.value = "each";
  monthDays.value = [recurrenceAnchor.value.getDate()];
  ordinal.value = 1;
  ordinalDay.value = recurrenceAnchor.value.getDay() || 7;
  monthWorkday.value = "first";
  yearMonth.value = recurrenceAnchor.value.getMonth() + 1;
  skipWeekends.value = false;
  recurrencePatternEnabled.value = false;
  ordinalDefaults.value = { monthly: false, yearly: false };
}

const customRepeatOpen = ref(false);
const weekStart = computed(() =>
  [0, 1, 6].includes(Number(props.settings.week_start))
    ? Number(props.settings.week_start)
    : 1,
);
const dayNames = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];
const repeatBasisChoices = [
  { value: "due_date", label: "By Due Dates" },
  { value: "completion_date", label: "By Completion Dates" },
];
const repeatFrequencyChoices = [
  { value: "daily", label: "Day" },
  { value: "weekly", label: "Week" },
  { value: "monthly", label: "Month" },
  { value: "yearly", label: "Year" },
];
const repeatWorkdayChoices = [
  { value: "first", label: "First workday" },
  { value: "last", label: "Last workday" },
];
const repeatMonthChoices = recurrenceMonths.map((label, index) => ({
  value: index + 1,
  label,
}));
const repeatWeekdayChoices = recurrenceWeekdays.map((day) => ({
  value: day,
  label: dayNames[day % 7],
}));
const reminderUnitChoices = computed(() =>
  reminderUnits.value.map((unit) => ({
    value: unit,
    label: unit[0].toUpperCase() + unit.slice(1),
  })),
);
const weekdayHeaders = computed(() =>
  Array.from({ length: 7 }, (_, index) => (weekStart.value + index) % 7),
);
const weekdayOptions = [1, 2, 3, 4, 5, 6, 7];
const monthLabel = computed(() =>
  month.value.toLocaleDateString("en", { month: "long", year: "numeric" }),
);
const weeks = computed(() => {
  const first = new Date(
    month.value.getFullYear(),
    month.value.getMonth(),
    1,
    12,
  );
  first.setDate(1 - ((first.getDay() - weekStart.value + 7) % 7));
  return Array.from({ length: 6 }, (_, week) =>
    Array.from({ length: 7 }, (_, day) => {
      const value = new Date(first);
      value.setDate(first.getDate() + week * 7 + day);
      return {
        value: dateString(value),
        day: value.getDate(),
        outside: value.getMonth() !== month.value.getMonth(),
      };
    }),
  );
});
const focusableDay = computed(() => {
  const preferred = date.value || today;
  return weeks.value.flat().some((day) => day.value === preferred)
    ? preferred
    : dateString(
        new Date(month.value.getFullYear(), month.value.getMonth(), 1, 12),
      );
});
function weekNumber(value: string) {
  const day = parseDate(value);
  day.setDate(day.getDate() + 3 - ((day.getDay() + 6) % 7));
  const firstThursday = new Date(day.getFullYear(), 0, 4, 12);
  return (
    1 +
    Math.round(
      ((day.getTime() - firstThursday.getTime()) / 86400000 -
        3 +
        ((firstThursday.getDay() + 6) % 7)) /
        7,
    )
  );
}
function prettyTime(value: string) {
  if (!value) return "";
  if (String(props.settings.time_format) !== "12") return value;
  const [hour, minute] = value.split(":").map(Number);
  return `${hour % 12 || 12}:${String(minute).padStart(2, "0")} ${hour < 12 ? "AM" : "PM"}`;
}
function localParts(instant: Date, timezone: string) {
  const formatter = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  });
  const parts = Object.fromEntries(
    formatter.formatToParts(instant).map((part) => [part.type, part.value]),
  );
  return {
    date: `${parts.year}-${parts.month}-${parts.day}`,
    time: `${parts.hour}:${parts.minute}`,
    wall: Date.UTC(
      Number(parts.year),
      Number(parts.month) - 1,
      Number(parts.day),
      Number(parts.hour),
      Number(parts.minute),
      Number(parts.second),
    ),
  };
}
// Derive the possible offsets around the selected wall time. This validates
// daylight-saving gaps and chooses the first occurrence of a repeated hour.
function zonedInstant(day: string, clock: string, timezone: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day) || !/^\d{2}:\d{2}$/.test(clock))
    throw new Error("Choose a valid date and time.");
  const [year, mon, datePart] = day.split("-").map(Number);
  const [hours, minutes] = clock.split(":").map(Number);
  const wall = Date.UTC(year, mon - 1, datePart, hours, minutes);
  const offsets = new Set<number>();
  for (let step = -8; step <= 8; step++) {
    const instant = wall + step * 6 * 3600000;
    offsets.add(localParts(new Date(instant), timezone).wall - instant);
  }
  const matches = [...offsets]
    .map((offset) => wall - offset)
    .filter((instant) => {
      const parts = localParts(new Date(instant), timezone);
      return parts.date === day && parts.time === clock;
    })
    .sort((a, b) => a - b);
  if (!matches.length)
    throw new Error(
      "This time does not exist in the selected time zone. Choose another time.",
    );
  return matches[0];
}
try {
  const due = props.task.due_date
    ? zonedInstant(
        props.task.due_date,
        props.task.due_time || "09:00",
        zone.value,
      )
    : null;
  for (const reminder of initialReminders) {
    const minutes =
      due === null ? NaN : (due - new Date(reminder).getTime()) / 60000;
    if (reminderPresets.value.some((preset) => preset.minutes === minutes))
      selectedOffsets.value.push(minutes);
    else {
      const parts = localParts(new Date(reminder), zone.value);
      customReminders.value.push({ date: parts.date, time: parts.time });
    }
  }
} catch {
  /* Existing content remains preserved until the user changes it. */
}
const reminderSummary = computed(() => {
  const count = selectedOffsets.value.length + customReminders.value.length;
  if (!count) return "";
  if (count === 1 && selectedOffsets.value.length === 1)
    return (
      reminderPresets.value.find(
        (preset) => preset.minutes === selectedOffsets.value[0],
      )?.label || "1 reminder"
    );
  return `${count} reminders`;
});
const repeatChoice = computed({
  get: () => {
    if (!repeatEnabled.value) return "none";
    if (
      customRepeat.value ||
      recurrencePatternEnabled.value ||
      interval.value !== 1 ||
      endKind.value !== "never" ||
      repeatMode.value !== "due_date"
    )
      return "custom";
    if (
      frequency.value === "weekly" &&
      weekdays.value.join(",") === "1,2,3,4,5"
    )
      return "weekdays";
    if (frequency.value === "weekly" && weekdays.value.length > 1)
      return "custom";
    return frequency.value;
  },
  set: (value: string) => {
    repeatEnabled.value = value !== "none";
    customRepeat.value = value === "custom";
    if (!repeatEnabled.value || customRepeat.value) return;
    interval.value = 1;
    endKind.value = "never";
    repeatMode.value = "due_date";
    frequency.value =
      value === "weekdays" ? "weekly" : (value as Recurrence["frequency"]);
    weekdays.value = value === "weekdays" ? [1, 2, 3, 4, 5] : [];
  },
});
const repeatSummary = computed(() => {
  if (!repeatEnabled.value) return "";
  const pattern = recurrencePatternSummary();
  if (pattern) return pattern;
  if (repeatChoice.value === "weekdays") return "Every weekday";
  const unit = {
    daily: "day",
    weekly: "week",
    monthly: "month",
    yearly: "year",
  }[frequency.value];
  return interval.value === 1
    ? `Every ${unit}`
    : `Every ${interval.value} ${unit}s`;
});
function ordinalDate(day: number) {
  const suffix =
    day % 100 >= 11 && day % 100 <= 13
      ? "th"
      : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[day % 10] ||
        "th";
  return `${day}${suffix}`;
}
function moveMonth(amount: number) {
  month.value = new Date(
    month.value.getFullYear(),
    month.value.getMonth() + amount,
    1,
    12,
  );
}
function goToday() {
  month.value = parseDate(today);
}
function pickDate(value: string) {
  error.value = "";
  if (tab.value === "duration" && endpoint.value === "start") {
    startDate.value = value;
    if (!date.value || date.value < value) date.value = value;
    endpoint.value = "end";
  } else date.value = value;
}
function quickDate(offset: number, tonight = false) {
  const chosen = addDays(today, offset);
  pickDate(chosen);
  month.value = parseDate(chosen);
  if (tonight) time.value = "18:00";
}
async function dayKey(event: KeyboardEvent, value: string) {
  const offset = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[
    event.key
  ];
  let target = value;
  if (offset !== undefined) target = addDays(value, offset);
  else if (event.key === "Home")
    target = addDays(
      value,
      -((parseDate(value).getDay() - weekStart.value + 7) % 7),
    );
  else if (event.key === "End")
    target = addDays(
      value,
      6 - ((parseDate(value).getDay() - weekStart.value + 7) % 7),
    );
  else if (event.key === "PageUp" || event.key === "PageDown") {
    const next = parseDate(value);
    next.setDate(1);
    next.setMonth(next.getMonth() + (event.key === "PageUp" ? -1 : 1));
    target = dateString(next);
  } else return;
  event.preventDefault();
  month.value = parseDate(target);
  await nextTick();
  root.value
    ?.querySelector<HTMLButtonElement>(`[data-date="${target}"]`)
    ?.focus();
}
async function togglePanel(panel: "time" | "reminder" | "repeat" | "end") {
  const next = expanded.value === panel ? null : panel;
  if (expanded.value === "reminder") cancelReminders();
  if (expanded.value === "end") cancelEnd();
  if (expanded.value === "repeat" && customRepeatOpen.value)
    cancelCustomRepeat();
  expanded.value = next;
  customReminderOpen.value = false;
  if (expanded.value === "reminder") captureReminders();
  if (expanded.value === "repeat") {
    captureCustomRepeat();
    customRepeatOpen.value = false;
  }
  if (expanded.value === "end") {
    endSnapshot = {
      kind: endKind.value,
      date: endDate.value,
      count: repeatCount.value,
    };
    endPanel.value = "menu";
  }
  if (!expanded.value) return;
  await nextTick();
  positionSubmenu();
  if (panel === "time") {
    timeInput.value?.focus();
    const suggestedTime = time.value || localParts(new Date(), zone.value).time;
    const [suggestedHour, suggestedMinute] = suggestedTime
      .split(":")
      .map(Number);
    const nearestSlot = `${String(suggestedHour).padStart(2, "0")}:${suggestedMinute >= 30 ? "30" : "00"}`;
    const selected = timeList.value?.querySelector<HTMLElement>(
      `[data-time="${nearestSlot}"]`,
    );
    if (selected && timeList.value)
      timeList.value.scrollTop = Math.max(
        0,
        selected.offsetTop - timeList.value.clientHeight / 2,
      );
  }
}
function positionSubmenu() {
  const anchor = root.value?.querySelector<HTMLElement>(
    `[data-date-option="${expanded.value}"]`,
  );
  if (!anchor || !submenu.value || !root.value) return;
  const row = anchor.getBoundingClientRect();
  const picker = root.value.getBoundingClientRect();
  const width = Math.min(
    customReminderOpen.value && expanded.value === "reminder"
      ? 276
      : picker.width - 8,
    window.innerWidth - 16,
  );
  const left = Math.max(
    8,
    Math.min(
      picker.left + (picker.width - width) / 2,
      window.innerWidth - width - 8,
    ),
  );
  const below = window.innerHeight - row.bottom - 8;
  const above = row.top - 8;
  const upward =
    below < Math.min(submenu.value.scrollHeight, 480) && above > below;
  const available = Math.max(100, upward ? above : below);
  submenuStyle.value = {
    left: `${left}px`,
    width: `${width}px`,
    maxHeight: `${available}px`,
    ...(upward
      ? { bottom: `${window.innerHeight - row.top - 2}px` }
      : { top: `${row.bottom - 2}px` }),
  };
}
function closeSubmenu() {
  const previous = expanded.value;
  expanded.value = null;
  if (previous)
    nextTick(() =>
      root.value
        ?.querySelector<HTMLButtonElement>(
          `[data-date-option="${previous}"] .date-option-main`,
        )
        ?.focus(),
    );
}
function outsidePointer(event: PointerEvent) {
  if (!(event.target instanceof Node) || !expanded.value) return;
  const row = root.value?.querySelector(
    `[data-date-option="${expanded.value}"]`,
  );
  if (submenu.value?.contains(event.target) || row?.contains(event.target))
    return;
  if (expanded.value === "reminder") cancelReminders();
  else if (expanded.value === "end") cancelEnd();
  else if (expanded.value === "repeat" && customRepeatOpen.value)
    cancelCustomRepeat();
  else closeSubmenu();
}
function viewportChanged(event?: Event) {
  if (!expanded.value) return;
  if (event?.target instanceof Node && submenu.value?.contains(event.target))
    return;
  positionSubmenu();
}
function chooseTime(value: string) {
  time.value = value;
  closeSubmenu();
}
let repeatSnapshot: {
  frequency: Recurrence["frequency"];
  interval: number;
  weekdays: number[];
  mode: Recurrence["mode"];
  enabled: boolean;
  custom: boolean;
  endKind: "never" | "date" | "count";
  endDate: string;
  count: number;
  details: RecurrenceDetailsSnapshot;
} | null = null;
function captureCustomRepeat() {
  repeatSnapshot = {
    frequency: frequency.value,
    interval: interval.value,
    weekdays: [...weekdays.value],
    mode: repeatMode.value,
    enabled: repeatEnabled.value,
    custom: customRepeat.value,
    endKind: endKind.value,
    endDate: endDate.value,
    count: repeatCount.value,
    details: captureRecurrenceDetails(),
  };
}
function cancelCustomRepeat() {
  if (repeatSnapshot) {
    frequency.value = repeatSnapshot.frequency;
    interval.value = repeatSnapshot.interval;
    weekdays.value = [...repeatSnapshot.weekdays];
    repeatMode.value = repeatSnapshot.mode;
    repeatEnabled.value = repeatSnapshot.enabled;
    customRepeat.value = repeatSnapshot.custom;
    endKind.value = repeatSnapshot.endKind;
    endDate.value = repeatSnapshot.endDate;
    repeatCount.value = repeatSnapshot.count;
    restoreRecurrenceDetails(repeatSnapshot.details);
  }
  customRepeatOpen.value = false;
  error.value = "";
  closeSubmenu();
}
function confirmCustomRepeat() {
  if (
    !Number.isInteger(interval.value) ||
    interval.value < 1 ||
    interval.value > 365
  ) {
    error.value = "Repeat interval must be between 1 and 365.";
    return;
  }
  customRepeat.value = true;
  repeatEnabled.value = true;
  customRepeatOpen.value = false;
  error.value = "";
  closeSubmenu();
}
function chooseRepeat(value: string) {
  const wasEnabled = repeatEnabled.value;
  repeatChoice.value = value;
  if (value === "custom") {
    customRepeatOpen.value = true;
    repeatEnabled.value = wasEnabled;
    if (!wasEnabled) {
      frequency.value = "weekly";
      weekdays.value = [recurrenceAnchor.value.getDay() || 7];
    }
  } else {
    resetRecurrenceDetails();
    closeSubmenu();
  }
}
const endMonth = ref(parseDate(endDate.value || today));
const endMonthLabel = computed(() =>
  endMonth.value.toLocaleDateString("en", { month: "long", year: "numeric" }),
);
const endWeeks = computed(() => {
  const first = new Date(
    endMonth.value.getFullYear(),
    endMonth.value.getMonth(),
    1,
    12,
  );
  first.setDate(1 - ((first.getDay() - weekStart.value + 7) % 7));
  return Array.from({ length: 6 }, (_, week) =>
    Array.from({ length: 7 }, (_, day) => {
      const value = new Date(first);
      value.setDate(first.getDate() + week * 7 + day);
      return {
        value: dateString(value),
        day: value.getDate(),
        outside: value.getMonth() !== endMonth.value.getMonth(),
      };
    }),
  );
});
function moveEndMonth(amount: number) {
  endMonth.value = new Date(
    endMonth.value.getFullYear(),
    endMonth.value.getMonth() + amount,
    1,
    12,
  );
}
function selectEndDate(value: string) {
  if (value < (date.value || today)) {
    error.value = "Choose an end date on or after the task date.";
    return;
  }
  endDate.value = value;
  endKind.value = "date";
  error.value = "";
  closeSubmenu();
}
async function endDateKey(event: KeyboardEvent, value: string) {
  const offsets: Record<string, number> = {
    ArrowLeft: -1,
    ArrowRight: 1,
    ArrowUp: -7,
    ArrowDown: 7,
  };
  let target: string;
  if (event.key in offsets) target = addDays(value, offsets[event.key]);
  else if (event.key === "Home")
    target = addDays(
      value,
      -((parseDate(value).getDay() - weekStart.value + 7) % 7),
    );
  else if (event.key === "End")
    target = addDays(
      value,
      6 - ((parseDate(value).getDay() - weekStart.value + 7) % 7),
    );
  else if (event.key === "PageUp" || event.key === "PageDown") {
    const next = parseDate(value);
    next.setDate(1);
    next.setMonth(next.getMonth() + (event.key === "PageUp" ? -1 : 1));
    target = dateString(next);
  } else return;
  event.preventDefault();
  event.stopPropagation();
  endMonth.value = parseDate(target);
  await nextTick();
  submenu.value
    ?.querySelector<HTMLButtonElement>(`[data-end-date="${target}"]`)
    ?.focus();
}
function chooseEnd(value: "never" | "date" | "count") {
  if (value === "never") {
    endKind.value = "never";
    closeSubmenu();
    return;
  }
  endPanel.value = value;
  if (value === "date") {
    if (!endDate.value) {
      const nextYear = parseDate(today);
      const currentMonth = nextYear.getMonth();
      nextYear.setFullYear(nextYear.getFullYear() + 1);
      if (nextYear.getMonth() !== currentMonth) nextYear.setDate(0);
      endDate.value = [dateString(nextYear), date.value || today]
        .sort()
        .at(-1)!;
    }
    endMonth.value = parseDate(endDate.value);
  }
  nextTick(() => {
    positionSubmenu();
    if (value === "date")
      submenu.value
        ?.querySelector<HTMLButtonElement>(`[data-end-date="${endDate.value}"]`)
        ?.focus();
    else submenu.value?.querySelector<HTMLInputElement>("input")?.focus();
  });
}
function cancelEnd() {
  endKind.value = endSnapshot.kind;
  endDate.value = endSnapshot.date;
  repeatCount.value = endSnapshot.count;
  error.value = "";
  closeSubmenu();
}
function confirmEnd() {
  if (endPanel.value === "count") {
    if (
      !Number.isInteger(repeatCount.value) ||
      repeatCount.value < 1 ||
      repeatCount.value > 1000000
    ) {
      error.value = "Choose between 1 and 1,000,000 occurrences.";
      return;
    }
    endKind.value = "count";
  } else if (endPanel.value === "date") {
    if (!endDate.value || endDate.value < (date.value || today)) {
      error.value = "Choose an end date on or after the task date.";
      return;
    }
    endKind.value = "date";
  }
  error.value = "";
  closeSubmenu();
}
function clearReminders() {
  selectedOffsets.value = [];
  customReminders.value = [];
  remindersChanged.value = true;
  closeSubmenu();
}
function menuKey(event: KeyboardEvent) {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  if (
    event.target instanceof HTMLInputElement ||
    event.target instanceof HTMLSelectElement
  )
    return;
  const items = [
    ...(submenu.value?.querySelectorAll<HTMLElement>(
      '[role="option"], [role="menuitem"], input[type="checkbox"]',
    ) || []),
  ];
  if (!items.length) return;
  event.preventDefault();
  const current = items.indexOf(document.activeElement as HTMLElement);
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : Math.max(
            0,
            Math.min(
              items.length - 1,
              current + (event.key === "ArrowDown" ? 1 : -1),
            ),
          );
  items[next]?.focus();
}
watch(
  [
    expanded,
    customReminderOpen,
    customRepeatOpen,
    customRepeat,
    monthlyMode,
    yearMonth,
    frequency,
    endKind,
    endPanel,
  ],
  () => nextTick(positionSubmenu),
);
onMounted(() => {
  unregisterEscapeLayer = registerEscapeLayer(escapeFromOutside);
  document.addEventListener("pointerdown", outsidePointer);
  window.addEventListener("resize", viewportChanged);
  document.addEventListener("scroll", viewportChanged, true);
});
onBeforeUnmount(() => {
  unregisterEscapeLayer?.();
  document.removeEventListener("pointerdown", outsidePointer);
  window.removeEventListener("resize", viewportChanged);
  document.removeEventListener("scroll", viewportChanged, true);
});
function toggleReminder(minutes: number) {
  remindersChanged.value = true;
  selectedOffsets.value = selectedOffsets.value.includes(minutes)
    ? selectedOffsets.value.filter((value) => value !== minutes)
    : [...selectedOffsets.value, minutes];
}
function addCustomReminder() {
  try {
    zonedInstant(customDate.value, customTime.value, zone.value);
    if (
      !customReminders.value.some(
        (value) =>
          value.date === customDate.value && value.time === customTime.value,
      )
    )
      customReminders.value.push({
        date: customDate.value,
        time: customTime.value,
      });
    remindersChanged.value = true;
    error.value = "";
  } catch (cause) {
    error.value =
      cause instanceof Error ? cause.message : "Choose a valid reminder.";
  }
}
function removeCustomReminder(index: number) {
  customReminders.value.splice(index, 1);
  remindersChanged.value = true;
}
function toggleWeekday(day: number) {
  weekdays.value = weekdays.value.includes(day)
    ? weekdays.value.filter((value) => value !== day)
    : [...weekdays.value, day].sort((a, b) => a - b);
}
function clearDate() {
  emit("save", {
    due_date: null,
    due_time: null,
    start_date: null,
    start_time: null,
    duration_minutes: null,
    recurrence: null,
    reminders: [],
  });
}
function recurrenceCalendarKey(
  rule: Pick<Recurrence, "frequency" | "interval" | "mode" | "weekdays"> &
    DetailedRecurrence,
) {
  return JSON.stringify([
    rule.frequency,
    rule.interval,
    rule.mode,
    [...(rule.weekdays || [])].sort((a, b) => a - b),
    [...(rule.month_days || [])].sort((a, b) => a - b),
    rule.ordinal_weekday?.ordinal ?? null,
    rule.ordinal_weekday?.weekday ?? null,
    rule.workday ?? null,
    rule.year_month ?? null,
    !!rule.skip_weekends,
  ]);
}
const initialDueDate = props.task.due_date;
const initialRecurrenceAnchor =
  props.task.recurrence?.anchor_date || initialDueDate;
const initialRecurrenceCalendar = props.task.recurrence
  ? recurrenceCalendarKey(props.task.recurrence)
  : null;
function recurrencePayload() {
  const rule = {
    frequency: frequency.value,
    interval: interval.value,
    weekdays: frequency.value === "weekly" ? weekdays.value : [],
    end_date: endKind.value === "date" ? endDate.value : null,
    count: endKind.value === "count" ? repeatCount.value : null,
    mode: repeatMode.value,
    ...recurrenceDetails(),
  };
  // Keep month-end/leap-day intent when editing an occurrence's time,
  // reminders, or end condition. A calendar/date change starts a new anchor.
  return {
    ...rule,
    anchor_date:
      date.value === initialDueDate &&
      recurrenceCalendarKey(rule) === initialRecurrenceCalendar
        ? initialRecurrenceAnchor
        : date.value || null,
  };
}
const previewDates = ref<Set<string>>(new Set());
const previewLoading = ref(false);
const previewError = ref("");
const previewAssumption = ref<"due_date" | "completion_on_due_date">(
  "due_date",
);
let previewGeneration = 0;
let previewTimer: ReturnType<typeof setTimeout> | undefined;
const previewInput = computed(() => {
  if (
    !repeatEnabled.value ||
    !date.value ||
    !Number.isInteger(interval.value) ||
    interval.value < 1 ||
    interval.value > 365
  )
    return null;
  if (
    endKind.value === "date" &&
    (!endDate.value || endDate.value < date.value)
  )
    return null;
  if (
    endKind.value === "count" &&
    (!Number.isInteger(repeatCount.value) ||
      repeatCount.value < 1 ||
      repeatCount.value > 1000000)
  )
    return null;
  const visibleDays = weeks.value.flat();
  return {
    due_date: date.value,
    recurrence: recurrencePayload(),
    timezone: zone.value,
    range_start: visibleDays[0].value,
    range_end: visibleDays.at(-1)!.value,
    limit: 128,
  };
});
watch(
  previewInput,
  (input) => {
    const generation = ++previewGeneration;
    clearTimeout(previewTimer);
    previewDates.value = new Set();
    previewError.value = "";
    previewLoading.value = !!input;
    if (!input) return;
    previewTimer = setTimeout(async () => {
      try {
        const result = await dispatch<{
          dates: string[];
          truncated: boolean;
          assumption: "due_date" | "completion_on_due_date";
        }>("recurrence.preview", input);
        if (generation !== previewGeneration) return;
        previewDates.value = new Set(result.dates);
        previewAssumption.value = result.assumption;
      } catch {
        if (generation !== previewGeneration) return;
        previewError.value =
          "Upcoming occurrences could not be previewed. Your task can still be saved.";
      } finally {
        if (generation === previewGeneration) previewLoading.value = false;
      }
    }, 120);
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  ++previewGeneration;
  clearTimeout(previewTimer);
});
function saveDate() {
  try {
    if (!date.value) {
      if (
        time.value ||
        repeatEnabled.value ||
        selectedOffsets.value.length ||
        customReminders.value.length ||
        tab.value === "duration"
      )
        throw new Error("Choose a date first.");
      clearDate();
      return;
    }
    const due = zonedInstant(date.value, time.value || "09:00", zone.value);
    const patch: Record<string, unknown> = {
      due_date: date.value,
      due_time: time.value || null,
      timezone: zone.value,
    };
    if (tab.value === "duration") {
      const start = zonedInstant(
        startDate.value,
        startTime.value || "00:00",
        zone.value,
      );
      const end = zonedInstant(date.value, time.value || "23:59", zone.value);
      const duration = Math.round((end - start) / 60000);
      if (duration < 1) throw new Error("End must be after Start.");
      if (duration > 525600)
        throw new Error("Duration cannot exceed one year.");
      Object.assign(patch, {
        start_date: startDate.value,
        start_time: startTime.value || null,
        duration_minutes: duration,
      });
    } else
      Object.assign(patch, {
        start_date: null,
        start_time: null,
        duration_minutes: null,
      });
    if (repeatEnabled.value) {
      if (
        !Number.isInteger(interval.value) ||
        interval.value < 1 ||
        interval.value > 365
      )
        throw new Error("Repeat interval must be between 1 and 365.");
      if (
        endKind.value === "date" &&
        (!endDate.value || endDate.value < date.value)
      )
        throw new Error("Choose an end date on or after the task date.");
      if (
        endKind.value === "count" &&
        (!Number.isInteger(repeatCount.value) ||
          repeatCount.value < 1 ||
          repeatCount.value > 1000000)
      )
        throw new Error("Choose between 1 and 1,000,000 occurrences.");
      patch.recurrence = recurrencePayload();
    } else patch.recurrence = null;
    if (remindersChanged.value) {
      patch.reminders = [
        ...new Set([
          ...selectedOffsets.value.map((minutes) =>
            new Date(
              !time.value && minutes % 1440 === 0
                ? zonedInstant(
                    addDays(date.value, -minutes / 1440),
                    "09:00",
                    zone.value,
                  )
                : due - minutes * 60000,
            ).toISOString(),
          ),
          ...customReminders.value.map((reminder) =>
            new Date(
              zonedInstant(reminder.date, reminder.time, zone.value),
            ).toISOString(),
          ),
        ]),
      ].sort();
    } else if (
      (!props.newTask || initialReminders.length > 0) &&
      props.task.due_date &&
      (props.task.due_date !== date.value ||
        (props.task.due_time || "") !== time.value ||
        (props.task.timezone || localZone) !== zone.value)
    ) {
      const oldDue = zonedInstant(
        props.task.due_date,
        props.task.due_time || "09:00",
        props.task.timezone || localZone,
      );
      patch.reminders = initialReminders.map((reminder) =>
        new Date(new Date(reminder).getTime() + due - oldDue).toISOString(),
      );
    }
    error.value = "";
    emit("save", patch);
  } catch (cause) {
    error.value =
      cause instanceof Error ? cause.message : "Choose a valid date and time.";
  }
}
function escape(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    for (const select of [...dateSelects.values()].reverse()) {
      if (select.closeOpenMenu()) return;
    }
    if (expanded.value === "reminder") cancelReminders();
    else if (expanded.value === "end") cancelEnd();
    else if (expanded.value === "repeat" && customRepeatOpen.value)
      cancelCustomRepeat();
    else if (expanded.value) closeSubmenu();
    else emit("close");
  }
}
</script>

<template>
  <div
    ref="root"
    class="task-date-picker"
    role="dialog"
    aria-label="Task date and reminders"
    @keydown="escape"
    @click.stop
  >
    <div class="date-segments" role="tablist" aria-label="Date type">
      <button
        type="button"
        role="tab"
        :aria-selected="tab === 'date'"
        :class="{ active: tab === 'date' }"
        @click="tab = 'date'"
      >
        Date
      </button>
      <button
        type="button"
        role="tab"
        :aria-selected="tab === 'duration'"
        :class="{ active: tab === 'duration' }"
        @click="tab = 'duration'"
      >
        Duration
      </button>
    </div>
    <div v-if="tab === 'duration'" class="duration-fields">
      <label :class="{ active: endpoint === 'start' }"
        ><span>Start</span
        ><input
          v-model="startDate"
          type="date"
          aria-label="Start date"
          @focus="endpoint = 'start'" /><input
          v-model="startTime"
          type="time"
          aria-label="Start time"
          @focus="endpoint = 'start'"
      /></label>
      <label :class="{ active: endpoint === 'end' }"
        ><span>End</span
        ><input
          v-model="date"
          type="date"
          aria-label="End date"
          @focus="endpoint = 'end'" /><input
          v-model="time"
          type="time"
          aria-label="End time"
          @focus="endpoint = 'end'"
      /></label>
    </div>
    <div class="date-shortcuts">
      <button
        type="button"
        aria-label="Today"
        title="Today"
        @click="quickDate(0)"
      >
        <PhSun :size="23" />
      </button>
      <button
        type="button"
        aria-label="Tomorrow"
        title="Tomorrow"
        @click="quickDate(1)"
      >
        <PhSunHorizon :size="23" />
      </button>
      <button
        type="button"
        aria-label="Next week"
        title="Next week"
        @click="quickDate(7)"
      >
        <svg
          width="22"
          height="22"
          viewBox="0 0 24 24"
          fill="none"
          aria-hidden="true"
        >
          <rect
            x="3.5"
            y="3.5"
            width="17"
            height="18"
            rx="4"
            stroke="currentColor"
            stroke-width="1.7"
          />
          <path
            d="M4 8h16M7 13h4M9 11v4M13 11h4l-3 6"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <button
        type="button"
        aria-label="Tonight"
        title="Tonight, 6:00 PM"
        @click="quickDate(0, true)"
      >
        <PhMoon :size="23" />
      </button>
    </div>
    <div class="calendar-heading">
      <strong>{{ monthLabel }}</strong>
      <button type="button" aria-label="Previous month" @click="moveMonth(-1)">
        <PhCaretLeft :size="14" />
      </button>
      <button type="button" aria-label="Current month" @click="goToday">
        <PhCircle :size="11" />
      </button>
      <button type="button" aria-label="Next month" @click="moveMonth(1)">
        <PhCaretRight :size="14" />
      </button>
    </div>
    <table
      class="date-calendar"
      :class="{ 'with-week-numbers': settings.show_week_numbers }"
      role="grid"
      :aria-label="monthLabel"
      :aria-busy="previewLoading"
    >
      <thead>
        <tr>
          <th v-if="settings.show_week_numbers" scope="col" class="week-number">
            W
          </th>
          <th
            v-for="day in weekdayHeaders"
            :key="day"
            scope="col"
            :aria-label="dayNames[day]"
          >
            {{ dayNames[day][0] }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(week, index) in weeks" :key="index">
          <td v-if="settings.show_week_numbers" class="week-number">
            {{ weekNumber(week[3].value) }}
          </td>
          <td
            v-for="day in week"
            :key="day.value"
            :aria-selected="
              day.value === date ||
              (tab === 'duration' && day.value === startDate)
            "
          >
            <button
              type="button"
              :data-date="day.value"
              :data-recurrence-preview="
                previewDates.has(day.value) ? 'true' : undefined
              "
              :aria-description="
                previewDates.has(day.value)
                  ? previewAssumption === 'completion_on_due_date'
                    ? 'Upcoming occurrence, assuming completion on each due date'
                    : 'Upcoming occurrence'
                  : undefined
              "
              :aria-label="
                parseDate(day.value).toLocaleDateString('en', {
                  weekday: 'long',
                  year: 'numeric',
                  month: 'long',
                  day: 'numeric',
                })
              "
              :aria-current="day.value === today ? 'date' : undefined"
              :tabindex="day.value === focusableDay ? 0 : -1"
              :class="{
                outside: day.outside,
                'recurrence-preview': previewDates.has(day.value),
                selected:
                  day.value === date ||
                  (tab === 'duration' && day.value === startDate),
                'in-range':
                  tab === 'duration' &&
                  day.value > startDate &&
                  day.value < date,
                today: day.value === today,
              }"
              @click="pickDate(day.value)"
              @keydown="dayKey($event, day.value)"
            >
              {{ day.day }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>
    <span v-if="previewError" class="sr-only" role="status">{{
      previewError
    }}</span>
    <div class="date-options">
      <div
        class="date-option"
        data-date-option="time"
        :class="{ expanded: expanded === 'time', 'has-value': time }"
      >
        <button
          type="button"
          class="date-option-main"
          aria-label="Time"
          :aria-expanded="expanded === 'time'"
          @click="togglePanel('time')"
        >
          <PhClock :size="17" /><span v-if="expanded !== 'time'">{{
            time ? prettyTime(time) : "Time"
          }}</span>
          <PhCaretRight
            v-if="expanded !== 'time'"
            :size="13"
            class="option-arrow"
          />
        </button>
        <input
          v-if="expanded === 'time'"
          ref="timeInput"
          v-model="time"
          class="inline-time"
          type="time"
          aria-label="Due time"
          @keydown.enter.prevent="closeSubmenu"
        />
        <button
          v-if="expanded === 'time'"
          class="clear-option"
          aria-label="Clear time"
          @click="
            time = '';
            closeSubmenu();
          "
        >
          <PhX :size="13" />
        </button>
      </div>
      <div
        class="date-option"
        data-date-option="reminder"
        :class="{
          expanded: expanded === 'reminder',
          'has-value': reminderSummary,
        }"
      >
        <button
          type="button"
          class="date-option-main"
          aria-label="Reminder"
          :aria-expanded="expanded === 'reminder'"
          @click="togglePanel('reminder')"
        >
          <PhAlarm :size="17" /><span>{{ reminderSummary || "Reminder" }}</span>
          <component
            v-if="
              expanded !== 'reminder' ||
              (!reminderSummary && !customReminderOpen)
            "
            :is="expanded === 'reminder' ? PhCaretDown : PhCaretRight"
            :size="13"
            class="option-arrow"
          />
        </button>
        <button
          v-if="
            expanded === 'reminder' && (reminderSummary || customReminderOpen)
          "
          class="clear-option"
          aria-label="Clear reminders"
          @click="clearReminders"
        >
          <PhX :size="13" />
        </button>
      </div>
      <div
        class="date-option"
        data-date-option="repeat"
        :class="{ expanded: expanded === 'repeat', 'has-value': repeatEnabled }"
      >
        <button
          type="button"
          class="date-option-main"
          aria-label="Repeat"
          :aria-expanded="expanded === 'repeat'"
          @click="togglePanel('repeat')"
        >
          <PhRepeat :size="17" /><span>{{ repeatSummary || "Repeat" }}</span>
          <component
            v-if="expanded !== 'repeat' || !repeatEnabled"
            :is="expanded === 'repeat' ? PhCaretDown : PhCaretRight"
            :size="13"
            class="option-arrow"
          />
        </button>
        <button
          v-if="expanded === 'repeat' && repeatEnabled"
          class="clear-option"
          aria-label="Clear repeat"
          @click="chooseRepeat('none')"
        >
          <PhX :size="13" />
        </button>
      </div>
      <div
        v-if="repeatEnabled"
        class="date-option"
        data-date-option="end"
        :class="{ expanded: expanded === 'end' }"
      >
        <button
          type="button"
          class="date-option-main"
          aria-label="End repeat"
          :aria-expanded="expanded === 'end'"
          @click="togglePanel('end')"
        >
          <PhRepeatOnce :size="17" /><span>{{ endSummary }}</span
          ><component
            :is="expanded === 'end' ? PhCaretDown : PhCaretRight"
            :size="13"
            class="option-arrow"
          />
        </button>
      </div>
      <div
        v-if="expanded"
        ref="submenu"
        class="date-submenu"
        :class="`submenu-${expanded}`"
        :style="submenuStyle"
        @keydown="menuKey"
        @click.stop
      >
        <template v-if="expanded === 'time'">
          <div
            ref="timeList"
            class="time-slot-list"
            role="listbox"
            aria-label="Choose time"
          >
            <button
              v-for="slot in times"
              :key="slot"
              type="button"
              role="option"
              :data-time="slot"
              :aria-selected="time === slot"
              @click="chooseTime(slot)"
            >
              <span>{{ prettyTime(slot) }}</span
              ><PhCheck v-if="time === slot" :size="17" />
            </button>
          </div>
          <div
            v-if="settings.show_timezone"
            class="date-option-body time-zone-fields"
          >
            <label v-if="settings.show_timezone" class="stacked-field"
              ><span>Time zone</span
              ><input
                v-model="zone"
                list="task-timezones"
                aria-label="Time zone"
                placeholder="Europe/Zurich" /><datalist id="task-timezones">
                <option
                  v-for="name in [
                    localZone,
                    'UTC',
                    'Europe/Zurich',
                    'Europe/London',
                    'America/New_York',
                    'America/Los_Angeles',
                    'Asia/Tokyo',
                    'Australia/Sydney',
                  ]"
                  :key="name"
                  :value="name"
                /></datalist
            ></label>
          </div>
        </template>
        <template v-else-if="expanded === 'reminder'">
          <div
            v-if="!customReminderOpen"
            class="reminder-menu"
            role="group"
            aria-label="Reminder options"
          >
            <label
              v-for="preset in reminderPresets"
              :key="preset.minutes"
              class="reminder-menu-choice"
              :class="{ selected: selectedOffsets.includes(preset.minutes) }"
            >
              <span
                >{{ preset.label }}
                <small v-if="preset.suffix">{{ preset.suffix }}</small></span
              >
              <input
                type="checkbox"
                :aria-label="`${preset.label}${preset.suffix ? ' ' + preset.suffix : ''}`"
                :checked="selectedOffsets.includes(preset.minutes)"
                @change="toggleReminder(preset.minutes)"
              />
              <PhCheck
                v-if="selectedOffsets.includes(preset.minutes)"
                :size="17"
              />
            </label>
            <button
              v-for="(reminder, index) in customReminders"
              :key="`${reminder.date}-${reminder.time}`"
              type="button"
              role="menuitem"
              class="selected-reminder"
              @click="removeCustomReminder(index)"
            >
              {{ customReminderLabel(reminder) }}<PhCheck :size="17" />
            </button>
            <div class="submenu-divider"></div>
            <button type="button" role="menuitem" @click="openCustomReminder">
              Custom
            </button>
            <div class="submenu-actions">
              <button type="button" @click="cancelReminders">Cancel</button
              ><button type="button" @click="confirmReminders">OK</button>
            </div>
          </div>
          <div
            v-else
            class="custom-reminder-form"
            :class="{ 'timed-reminder': time }"
            role="group"
            aria-label="Custom reminder"
          >
            <div class="custom-reminder-top" :class="{ timed: time }">
              <input
                v-if="time"
                v-model.number="reminderAmount"
                type="number"
                min="0"
                max="10000"
                aria-label="Reminder amount"
              />
              <DatePickerSelect
                :ref="
                  (instance) => rememberDateSelect('reminderUnit', instance)
                "
                :model-value="reminderUnit"
                :options="reminderUnitChoices"
                label="Reminder unit"
                :menu-min-width="122"
                @update:model-value="
                  reminderUnit = $event as 'minute' | 'hour' | 'day' | 'week'
                "
              />
            </div>
            <label v-if="!time" class="custom-reminder-row"
              ><span
                >{{ reminderUnit[0].toUpperCase() + reminderUnit.slice(1) }} in
                advance</span
              ><input
                v-model.number="reminderAmount"
                type="number"
                min="0"
                max="10000"
                aria-label="Reminder amount"
            /></label>
            <label v-if="!time" class="custom-reminder-row"
              ><span>Remind me at</span
              ><input
                v-model="customTime"
                type="time"
                aria-label="Custom reminder time"
            /></label>
            <div class="custom-reminder-preview">
              <span
                >{{ reminderAmount }} {{ reminderUnit
                }}{{ reminderAmount === 1 ? "" : "s" }} early</span
              ><strong v-if="customReminderPreview">{{
                time
                  ? `Remind at ${prettyTime(customReminderPreview.time)} on ${parseDate(customReminderPreview.date).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" })}`
                  : `Remind at ${parseDate(customReminderPreview.date).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" })} at ${prettyTime(customReminderPreview.time)}`
              }}</strong
              ><strong v-else>Choose a valid reminder</strong>
            </div>
            <div class="submenu-actions">
              <button type="button" @click="cancelReminders">Cancel</button
              ><button type="button" @click="confirmCustomReminder">OK</button>
            </div>
          </div>
        </template>
        <template v-else-if="expanded === 'repeat'">
          <div
            v-if="!customRepeatOpen"
            class="repeat-menu"
            role="menu"
            aria-label="Repeat frequency"
          >
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('daily')"
            >
              Daily<PhCheck v-if="repeatChoice === 'daily'" :size="17" />
            </button>
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('weekly')"
            >
              Weekly
              <small
                >({{
                  dayNames[parseDate(date || today).getDay()].slice(0, 3)
                }})</small
              ><PhCheck v-if="repeatChoice === 'weekly'" :size="17" />
            </button>
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('monthly')"
            >
              Monthly
              <small
                >({{ ordinalDate(parseDate(date || today).getDate()) }})</small
              ><PhCheck v-if="repeatChoice === 'monthly'" :size="17" />
            </button>
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('yearly')"
            >
              Yearly
              <small
                >({{
                  parseDate(date || today).toLocaleDateString("en-GB", {
                    day: "numeric",
                    month: "short",
                  })
                }})</small
              ><PhCheck v-if="repeatChoice === 'yearly'" :size="17" />
            </button>
            <div class="submenu-divider"></div>
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('weekdays')"
            >
              Every Weekday <small>(Mon - Fri)</small
              ><PhCheck v-if="repeatChoice === 'weekdays'" :size="17" />
            </button>
            <div class="submenu-divider"></div>
            <button
              type="button"
              role="menuitem"
              @click="chooseRepeat('custom')"
            >
              Custom<PhCheck v-if="repeatChoice === 'custom'" :size="17" />
            </button>
          </div>
          <div
            v-else
            class="custom-repeat-panel"
            role="group"
            :class="{
              'year-each-pattern':
                frequency === 'yearly' && monthlyMode === 'each',
            }"
            aria-label="Custom repeat"
          >
            <DatePickerSelect
              :ref="(instance) => rememberDateSelect('repeatMode', instance)"
              :model-value="repeatMode"
              :options="repeatBasisChoices"
              label="Repeat based on"
              class="repeat-basis"
              @update:model-value="repeatMode = $event as Recurrence['mode']"
            />
            <div class="custom-repeat-every">
              <label class="repeat-quantity"
                ><span>Every</span
                ><input
                  v-model.number="interval"
                  type="number"
                  min="1"
                  max="365"
                  aria-label="Repeat interval"
              /></label>
              <DatePickerSelect
                :ref="(instance) => rememberDateSelect('frequency', instance)"
                :model-value="frequency"
                :options="repeatFrequencyChoices"
                label="Custom repeat unit"
                @update:model-value="
                  changeCustomFrequency($event as Recurrence['frequency'])
                "
              />
            </div>
            <div
              v-if="frequency === 'weekly'"
              class="custom-repeat-weekdays"
              role="group"
              aria-label="Repeat on weekdays"
            >
              <button
                v-for="day in recurrenceWeekdays"
                :key="day"
                type="button"
                :aria-label="dayNames[day % 7]"
                :aria-pressed="selectedRepeatWeekdays.includes(day)"
                :class="{ selected: selectedRepeatWeekdays.includes(day) }"
                @click="toggleCustomWeekday(day)"
              >
                {{ dayNames[day % 7][0] }}
              </button>
            </div>
            <template v-if="frequency === 'monthly' || frequency === 'yearly'">
              <div
                class="custom-repeat-segments"
                role="tablist"
                aria-label="Repeat pattern"
              >
                <button
                  type="button"
                  role="tab"
                  :aria-selected="monthlyMode === 'each'"
                  :class="{ active: monthlyMode === 'each' }"
                  @click="chooseMonthlyMode('each')"
                >
                  Each
                </button>
                <button
                  type="button"
                  role="tab"
                  :aria-selected="monthlyMode === 'ordinal'"
                  :class="{ active: monthlyMode === 'ordinal' }"
                  @click="chooseMonthlyMode('ordinal')"
                >
                  On the
                </button>
                <button
                  v-if="frequency === 'monthly'"
                  type="button"
                  role="tab"
                  :aria-selected="monthlyMode === 'workday'"
                  :class="{ active: monthlyMode === 'workday' }"
                  @click="chooseMonthlyMode('workday')"
                >
                  Workday
                </button>
              </div>
              <template v-if="monthlyMode === 'each'">
                <div v-if="frequency === 'yearly'" class="repeat-year-heading">
                  <span>{{ recurrenceMonths[yearMonth - 1] }}</span
                  ><button
                    type="button"
                    aria-label="Previous repeat month"
                    @click="changeYearMonth(yearMonth - 1)"
                  >
                    <PhCaretLeft :size="14" /></button
                  ><button
                    type="button"
                    aria-label="Next repeat month"
                    @click="changeYearMonth(yearMonth + 1)"
                  >
                    <PhCaretRight :size="14" />
                  </button>
                </div>
                <div
                  class="repeat-month-days"
                  :class="{ yearly: frequency === 'yearly' }"
                  role="group"
                  :aria-label="
                    frequency === 'yearly'
                      ? 'Repeat on day of year'
                      : 'Repeat on days of month'
                  "
                >
                  <button
                    v-for="day in repeatCalendarDays"
                    :key="day"
                    type="button"
                    :aria-label="`Repeat on day ${day}`"
                    :aria-pressed="monthDays.includes(day)"
                    :class="{ selected: monthDays.includes(day) }"
                    @click="selectMonthDay(day)"
                  >
                    {{ day }}
                  </button>
                  <button
                    v-if="frequency === 'monthly'"
                    type="button"
                    class="last-day"
                    aria-label="Repeat on last day of month"
                    :aria-pressed="monthDays.includes(-1)"
                    :class="{ selected: monthDays.includes(-1) }"
                    @click="selectMonthDay(-1)"
                  >
                    Last Day
                  </button>
                </div>
                <label
                  v-if="frequency === 'monthly'"
                  class="repeat-skip-weekends"
                  ><input
                    v-model="skipWeekends"
                    type="checkbox"
                    aria-label="Skip Weekends"
                  /><span>Skip Weekends</span></label
                >
              </template>
              <template v-else-if="monthlyMode === 'ordinal'">
                <DatePickerSelect
                  :ref="(instance) => rememberDateSelect('yearMonth', instance)"
                  v-if="frequency === 'yearly'"
                  :model-value="yearMonth"
                  :options="repeatMonthChoices"
                  label="Repeat month"
                  class="repeat-year-select"
                  @update:model-value="changeYearMonth(Number($event))"
                />
                <div class="repeat-ordinal-row">
                  <DatePickerSelect
                    :ref="(instance) => rememberDateSelect('ordinal', instance)"
                    :model-value="ordinal"
                    :options="recurrenceOrdinals"
                    label="Repeat ordinal"
                    @update:model-value="ordinal = Number($event)"
                  />
                  <DatePickerSelect
                    :ref="
                      (instance) => rememberDateSelect('ordinalDay', instance)
                    "
                    :model-value="ordinalDay"
                    :options="repeatWeekdayChoices"
                    label="Repeat weekday"
                    @update:model-value="ordinalDay = Number($event)"
                  />
                </div>
              </template>
              <DatePickerSelect
                :ref="
                  (instance) => rememberDateSelect('monthWorkday', instance)
                "
                v-else
                :model-value="monthWorkday"
                :options="repeatWorkdayChoices"
                label="Repeat workday"
                class="repeat-workday-select"
                @update:model-value="monthWorkday = $event as 'first' | 'last'"
              />
            </template>
            <div class="custom-repeat-actions">
              <button type="button" @click="cancelCustomRepeat">Cancel</button
              ><button
                type="button"
                class="custom-repeat-confirm"
                @click="
                  commitRecurrenceDetails();
                  confirmCustomRepeat();
                "
              >
                OK
              </button>
            </div>
          </div>
        </template>
        <template v-else-if="expanded === 'end'">
          <div
            v-if="endPanel === 'menu'"
            class="end-menu"
            role="menu"
            aria-label="End repeat options"
          >
            <button type="button" role="menuitem" @click="chooseEnd('never')">
              Endlessly<PhCheck v-if="endKind === 'never'" :size="17" />
            </button>
            <button type="button" role="menuitem" @click="chooseEnd('date')">
              End by date<PhCheck v-if="endKind === 'date'" :size="17" />
            </button>
            <button type="button" role="menuitem" @click="chooseEnd('count')">
              End by count<PhCheck v-if="endKind === 'count'" :size="17" />
            </button>
          </div>
          <div v-else-if="endPanel === 'date'" class="end-date-panel">
            <div class="calendar-heading">
              <strong>{{ endMonthLabel }}</strong
              ><button
                type="button"
                aria-label="Previous end month"
                @click="moveEndMonth(-1)"
              >
                <PhCaretLeft :size="14" /></button
              ><button
                type="button"
                aria-label="Next end month"
                @click="moveEndMonth(1)"
              >
                <PhCaretRight :size="14" />
              </button>
            </div>
            <table
              class="date-calendar"
              role="grid"
              :aria-label="`Repeat ends, ${endMonthLabel}`"
            >
              <thead>
                <tr>
                  <th
                    v-for="day in weekdayHeaders"
                    :key="day"
                    scope="col"
                    :aria-label="dayNames[day]"
                  >
                    {{ dayNames[day][0] }}
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(week, index) in endWeeks" :key="index">
                  <td
                    v-for="day in week"
                    :key="day.value"
                    :aria-selected="endDate === day.value"
                  >
                    <button
                      type="button"
                      :data-end-date="day.value"
                      :class="{
                        outside: day.outside,
                        selected: endDate === day.value,
                      }"
                      :aria-label="
                        parseDate(day.value).toLocaleDateString('en', {
                          weekday: 'long',
                          year: 'numeric',
                          month: 'long',
                          day: 'numeric',
                        })
                      "
                      :tabindex="endDate === day.value ? 0 : -1"
                      @click="selectEndDate(day.value)"
                      @keydown="endDateKey($event, day.value)"
                    >
                      {{ day.day }}
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else class="end-repeat-form">
            <div v-if="endPanel === 'count'" class="repeat-count-row">
              <input
                v-model.number="repeatCount"
                type="number"
                min="1"
                max="1000000"
                aria-label="Repeat occurrences"
              /><span class="repeat-count-stepper"
                ><button
                  type="button"
                  aria-label="Increase repeat count"
                  @click="repeatCount = Math.min(1000000, repeatCount + 1)"
                >
                  <PhCaretDown :size="16" class="up" /></button
                ><button
                  type="button"
                  aria-label="Decrease repeat count"
                  @click="repeatCount = Math.max(1, repeatCount - 1)"
                >
                  <PhCaretDown :size="16" /></button></span
              ><span>Repeat Times</span>
            </div>
            <div class="submenu-actions">
              <button type="button" @click="cancelEnd">Cancel</button
              ><button type="button" @click="confirmEnd">OK</button>
            </div>
          </div>
        </template>
      </div>
    </div>
    <p v-if="error" class="date-error" role="alert">{{ error }}</p>
    <div class="date-actions">
      <button type="button" @click="clearDate">Clear</button
      ><button type="button" class="date-confirm" @click="saveDate">OK</button>
    </div>
  </div>
</template>

<style scoped>
.task-date-picker {
  --date-accent: var(--control-accent, #303c70);
  width: 260px;
  max-width: calc(100vw - 24px);
  max-height: min(710px, calc(100vh - 80px));
  overflow-y: auto;
  padding: 12px 16px;
  border-radius: 13px;
  border: 0;
  background: var(--popover, #fff);
  color: var(--text, #202020);
  box-shadow: 0 6px 22px #0003;
  font-size: 14px;
  text-align: left;
}
.task-date-picker button,
.task-date-picker input,
.task-date-picker select {
  font: inherit;
}
.task-date-picker button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
  padding: 0;
}
.task-date-picker button:focus-visible,
.task-date-picker input:focus-visible,
.task-date-picker select:focus-visible {
  outline: 2px solid var(--date-accent);
  outline-offset: 2px;
}
.date-segments {
  display: flex;
  height: 30px;
  padding: 2px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--text, #202020) 6%, transparent);
}
.date-segments button {
  flex: 1;
  color: var(--secondary, #757575);
  border-radius: 8px;
}
.date-segments button.active {
  color: var(--text, #202020);
  background: var(--popover, #fff);
  box-shadow: 0 1px 2px #00000004;
}
.date-shortcuts {
  display: flex;
  justify-content: space-between;
  margin: 21px 0 17px;
  padding: 0 2px;
}
.date-shortcuts button {
  position: relative;
  width: 27px;
  height: 28px;
  color: var(--secondary, #777);
  border-radius: 6px;
}
.date-shortcuts button:hover,
.calendar-heading button:hover {
  background: color-mix(in srgb, var(--text, #202020) 5%, transparent);
}
.calendar-heading {
  display: flex;
  align-items: center;
  margin-bottom: 0;
  gap: 2px;
  min-height: 24px;
}
.calendar-heading strong {
  margin-right: auto;
  font-size: 14px;
  font-weight: 500;
}
.calendar-heading button {
  width: 22px;
  height: 24px;
  border-radius: 5px;
}
.date-calendar {
  table-layout: fixed;
  width: 100%;
  border-collapse: collapse;
  margin: 0 -6px 7px;
  width: calc(100% + 12px);
}
.date-calendar th {
  height: 27px;
  font-size: 11px;
  color: var(--secondary, #aaa);
  font-weight: 400;
}
.date-calendar td {
  height: 30px;
  padding: 0;
  text-align: center;
}
.date-calendar td button {
  border-radius: 50%;
  width: 28px;
  height: 28px;
  font-size: 13px;
  line-height: 1;
}
.date-calendar td button:hover {
  background: color-mix(in srgb, var(--date-accent) 12%, transparent);
}
.date-calendar td button.outside {
  color: var(--secondary, #aaa);
  opacity: 0.7;
}
.date-calendar td button.recurrence-preview:not(.selected) {
  background: color-mix(in srgb, var(--date-accent) 50%, var(--popover, white));
  color: var(--text);
  opacity: 1;
}
.date-calendar td button.selected,
.repeat-weekdays button.selected {
  background: var(--date-accent);
  color: white;
  opacity: 1;
}
.date-calendar td button.in-range {
  background: color-mix(in srgb, var(--date-accent) 10%, transparent);
  border-radius: 4px;
}
.date-calendar td button.today:not(.selected):not(.recurrence-preview) {
  color: var(--date-accent);
  font-weight: 600;
}
.date-calendar .week-number {
  width: 17px;
  color: var(--secondary, #999);
  font-size: 9px;
}
.date-calendar.with-week-numbers {
  margin-left: -9px;
  width: calc(100% + 18px);
}
.date-options {
  margin: 6px -12px 0;
}
.date-option {
  position: relative;
  width: 100%;
  height: 36px;
  border-radius: 8px;
}
.date-option-main {
  width: 100%;
  height: 36px;
  gap: 10px;
  padding: 7px 12px !important;
  justify-content: flex-start !important;
  border-radius: 8px;
}
.date-option.expanded.has-value .date-option-main {
  padding-right: 36px !important;
}
.date-option-main > svg:first-child {
  color: var(--secondary, #888);
  flex-shrink: 0;
}
.date-option-main > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.date-option-main .option-arrow {
  margin-left: auto;
  color: var(--secondary, #888);
  flex-shrink: 0;
}
.date-option.has-value .date-option-main {
  color: var(--date-accent);
}
.date-option.has-value .date-option-main > svg:first-child {
  color: var(--date-accent);
}
.date-option:hover,
.date-option.expanded {
  background: color-mix(in srgb, var(--text, #202020) 4%, transparent);
}
.inline-time {
  position: absolute;
  top: 0;
  left: 38px;
  width: calc(100% - 74px);
  height: 36px;
  border: 0;
  background: transparent;
  color: var(--date-accent);
  padding: 0;
}
.inline-time::-webkit-calendar-picker-indicator {
  display: none;
}
.inline-time:focus-visible {
  outline: none !important;
}
.clear-option {
  position: absolute;
  right: 4px;
  top: 6px;
  width: 24px;
  height: 24px;
  color: var(--secondary, #888) !important;
  border-radius: 5px;
}
.clear-option:hover {
  background: var(--hover);
}
.date-submenu {
  position: fixed;
  z-index: 220;
  border-radius: 12px;
  background: var(--popover, #fff);
  box-shadow: 0 4px 20px #0003;
  padding: 4px 0;
  overflow-y: auto;
  color: var(--text);
  font-size: 14px;
}
.date-submenu button[role="option"],
.date-submenu button[role="menuitem"],
.reminder-menu-choice {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  height: 34px;
  min-height: 34px;
  padding: 0 16px;
  text-align: left;
  justify-content: flex-start;
}
.date-submenu button[role="option"] svg,
.date-submenu button[role="menuitem"] svg {
  margin-left: auto;
  color: var(--date-accent);
}
.date-submenu button[role="option"]:hover,
.date-submenu button[role="menuitem"]:hover,
.reminder-menu-choice:hover {
  background: var(--hover);
}
.date-submenu button[aria-selected="true"],
.reminder-menu-choice.selected {
  color: var(--date-accent);
}
.date-submenu small {
  color: var(--muted);
  font-size: inherit;
}
.time-slot-list {
  max-height: 272px;
  overflow-y: auto;
}
.reminder-menu-choice {
  position: relative;
  cursor: pointer;
}
.reminder-menu-choice input {
  position: absolute;
  right: 16px;
  width: 17px;
  height: 17px;
  opacity: 0;
  margin: 0;
}
.reminder-menu-choice input:focus-visible {
  outline: none;
}
.reminder-menu-choice:focus-within {
  outline: 2px solid var(--date-accent);
  outline-offset: -2px;
}
.reminder-menu-choice svg {
  margin-left: auto;
  color: var(--date-accent);
  pointer-events: none;
}
.submenu-back {
  gap: 5px;
  justify-content: flex-start !important;
  min-height: 28px;
  color: var(--secondary) !important;
}
.submenu-done {
  min-height: 28px;
  background: var(--date-accent) !important;
  color: white !important;
  border-radius: 6px;
}
:global(:root:not([data-theme="dark"])) .calendar-heading strong {
  color: #202020;
}
:global(:root:not([data-theme="dark"])) .date-calendar th,
:global(:root:not([data-theme="dark"])) .date-calendar td button.outside {
  color: #aaa;
  opacity: 1;
}
:global(:root:not([data-theme="dark"]))
  .date-option:not(.has-value)
  .date-option-main {
  color: #484848;
}
:global(:root:not([data-theme="dark"])) .date-option-main > svg:first-child {
  color: #888;
}
:global(:root:not([data-theme="dark"]))
  .date-option.has-value
  .date-option-main
  > svg:first-child {
  color: var(--date-accent);
}
.date-option-body {
  padding: 8px 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  font-size: 12px;
}
.date-option-body input:not([type="checkbox"]),
.date-option-body select,
.duration-fields input {
  min-width: 0;
  border: 1px solid color-mix(in srgb, var(--text, #202020) 15%, transparent);
  background: var(--popover, #fff);
  border-radius: 5px;
  color: inherit;
  padding: 5px 6px;
  height: 29px;
}
.date-option-body select {
  max-width: 100%;
}
.field-row,
.repeat-every {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.field-row input[type="number"] {
  width: 76px;
}
.stacked-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.plain-action {
  color: var(--date-accent) !important;
  justify-content: flex-start !important;
  font-size: 12px !important;
}
.muted-action {
  color: var(--secondary, #888) !important;
}
.reminder-choice {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}
.reminder-choice input {
  width: 13px;
  height: 13px;
  margin: 0;
  accent-color: var(--date-accent);
}
.custom-reminder {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.custom-reminder button {
  padding: 3px !important;
  color: var(--secondary, #888);
}
.reminder-custom-fields {
  display: flex;
  gap: 5px;
}
.reminder-custom-fields input[type="date"] {
  flex: 1;
  width: 119px;
}
.reminder-custom-fields input[type="time"] {
  width: 83px;
}
.repeat-every input {
  width: 46px;
}
.repeat-every select {
  flex: 1;
}
.repeat-weekdays {
  display: flex;
  justify-content: space-between;
}
.repeat-weekdays button {
  width: 25px;
  height: 25px;
  border-radius: 50%;
  font-size: 11px;
  background: color-mix(in srgb, var(--text, #202020) 5%, transparent);
}
.duration-fields {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 12px;
}
.duration-fields label {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
  border-radius: 6px;
  border: 1px solid transparent;
}
.duration-fields label.active {
  border-color: color-mix(in srgb, var(--date-accent) 35%, transparent);
}
.duration-fields label > span {
  width: 27px;
  font-size: 11px;
}
.duration-fields input {
  padding: 3px;
  font-size: 10px;
  height: 25px;
}
.duration-fields input[type="date"] {
  width: 105px;
}
.duration-fields input[type="time"] {
  width: 67px;
}
.date-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
.date-actions button {
  height: 28px;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--text, #202020) 14%, transparent);
  flex: 1;
}
.date-actions .date-confirm {
  color: white;
  background: var(--date-accent);
  border-color: var(--date-accent);
}
.date-error {
  margin: 8px 0 0;
  color: #c73636;
  font-size: 12px;
  line-height: 1.35;
}

.submenu-divider {
  height: 1px;
  background: var(--border);
  margin: 4px 0;
}
.submenu-actions {
  display: flex;
  gap: 8px;
  margin: 6px 18px 8px;
}
.submenu-actions button {
  flex: 1;
  min-width: 0;
  height: 24px;
  border-radius: 8px;
  border: 1px solid var(--border);
}
.submenu-actions button:last-child {
  background: var(--date-accent);
  color: white;
  border-color: var(--date-accent);
}
.submenu-reminder {
  padding-top: 8px;
  padding-bottom: 4px;
}
.custom-reminder-form {
  min-height: 180px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 12px 8px;
  font-size: 14px;
}
.custom-reminder-row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 24px;
}
.custom-reminder-row > span {
  flex: 0 0 94px;
  white-space: nowrap;
}
.custom-reminder-row input {
  min-width: 0;
  flex: 1;
  height: 24px;
  text-align: center;
  padding: 1px 5px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--popover);
  color: var(--text);
}
.custom-reminder-row input[type="time"] {
  border-color: transparent;
  background: color-mix(in srgb, var(--text) 5%, transparent);
}
.custom-reminder-row input[type="time"]::-webkit-calendar-picker-indicator {
  display: none;
}
.custom-reminder-preview {
  margin: 2px 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 12px;
  line-height: 15px;
  color: var(--secondary);
}
.custom-reminder-preview strong {
  font-weight: 400;
  color: var(--date-accent);
}
.custom-reminder-form .submenu-actions {
  margin: 2px 0 0;
}

.end-repeat-form {
  padding: 8px 10px 6px;
}
.repeat-count-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}
.repeat-count-row input {
  min-width: 0;
  flex: 1;
  width: 105px;
  height: 24px;
  border-radius: 7px;
  border: 0;
  background: var(--input);
  color: var(--text);
  text-align: center;
  appearance: textfield;
}
.repeat-count-row input::-webkit-inner-spin-button {
  appearance: none;
}
.repeat-count-stepper {
  display: flex;
  flex-direction: column;
  width: 20px;
  height: 24px;
  background: var(--input);
  border-radius: 5px;
}
.repeat-count-stepper button {
  height: 12px;
}
.repeat-count-stepper .up {
  transform: rotate(180deg);
}
.end-repeat-form .submenu-actions {
  margin: 14px 0 0;
}
.end-repeat-form > input[type="date"] {
  width: 100%;
  height: 28px;
  padding: 3px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  background: var(--popover);
}

/* The enclosing .date-submenu is 252px wide; its 4px vertical padding is
   compensated here, making the captured panel content top inset 13px. */
.custom-repeat-panel {
  padding: 9px 16px 12px;
  color: var(--text, #202020);
  font-size: 14px;
}
.custom-repeat-every {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 8px;
  margin-top: 12px;
}
.repeat-quantity {
  display: flex;
  align-items: center;
  min-width: 0;
  height: 24px;
  gap: 8px;
  padding: 0 8px;
  border-radius: 6px;
  background: var(--input, #f2f2f2);
}
.repeat-quantity input {
  width: 38px;
  min-width: 0;
  margin-left: auto;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: right;
  appearance: textfield;
  -moz-appearance: textfield;
  font: inherit;
}
.repeat-quantity input::-webkit-inner-spin-button,
.repeat-quantity input::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}
.custom-repeat-weekdays {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin: 12px -6px 0;
}
.custom-repeat-weekdays button {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  font-size: 13px;
  padding: 0;
}
.custom-repeat-panel button.selected {
  color: white;
  background: var(--date-accent, #303c70);
}
.custom-repeat-segments {
  display: flex;
  align-items: center;
  height: 26px;
  margin-top: 12px;
  padding: 2px;
  border-radius: 8px;
  background: var(--input, #f2f2f2);
}
.custom-repeat-segments button {
  flex: 1;
  height: 22px;
  min-width: 0;
  border-radius: 6px;
  color: var(--secondary, #777);
  font-size: 14px;
  white-space: nowrap;
}
.custom-repeat-segments button.active {
  color: var(--text, #202020);
  background: var(--popover, #fff);
}
.repeat-month-days {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  align-items: center;
  justify-items: center;
  grid-auto-rows: 35px;
  margin: 7px -11px 0;
}
.repeat-month-days button {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  font-size: 13px;
  padding: 0;
}
.repeat-month-days .last-day {
  grid-column: 4 / span 2;
  justify-self: start;
  margin-left: 5px;
  width: 61px;
  border-radius: 12px;
  white-space: nowrap;
}
.repeat-skip-weekends {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 5px 2px 0;
  height: 24px;
}
.repeat-skip-weekends input {
  appearance: none;
  width: 13px;
  height: 13px;
  border: 1px solid var(--muted, #ccc);
  border-radius: 3px;
  margin: 0;
  background: var(--popover, #fff);
  cursor: pointer;
}
.repeat-skip-weekends input:checked {
  background: var(--date-accent, #303c70);
  border-color: var(--date-accent, #303c70);
}
.repeat-skip-weekends input:checked::after {
  content: "";
  display: block;
  width: 4px;
  height: 7px;
  border: solid white;
  border-width: 0 1.5px 1.5px 0;
  transform: rotate(45deg);
  margin: 1px 0 0 4px;
}
.repeat-year-heading {
  display: flex;
  align-items: center;
  height: 24px;
  margin-top: 12px;
}
.repeat-year-heading > span {
  margin-right: auto;
}
.repeat-year-heading button {
  width: 24px;
  height: 24px;
}
.repeat-month-days.yearly {
  margin-top: -2px;
}
.repeat-year-select,
.repeat-ordinal-row,
.repeat-workday-select {
  margin-top: 12px;
}
.repeat-ordinal-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 8px;
}
.custom-repeat-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 20px;
}
.custom-repeat-actions button {
  flex: 1;
  height: 24px;
  border: 1px solid var(--border, #e4e4e4);
  border-radius: 6px;
  color: inherit;
  font-size: 14px;
}
.custom-repeat-actions .custom-repeat-confirm {
  color: white;
  background: var(--date-accent, #303c70);
  border-color: var(--date-accent, #303c70);
}
.year-each-pattern .custom-repeat-actions {
  margin-top: 10px;
}

.custom-reminder-form.timed-reminder {
  min-height: 116px;
}
.custom-reminder-form input[type="number"]::-webkit-inner-spin-button,
.custom-reminder-form input[type="number"]::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}
.custom-reminder-top.timed {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 8px;
}
.custom-reminder-top > input {
  min-width: 0;
  width: 100%;
  height: 24px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--popover);
  color: var(--text);
  padding: 0 8px;
}

.end-date-panel {
  height: 244px;
  padding: 8px 12px;
}
.end-date-panel .date-calendar {
  margin-top: 0;
  margin-bottom: 0;
}
.end-date-panel .date-calendar th {
  height: 24px;
}
.end-date-panel .date-calendar td {
  height: 30px;
}

@media (max-height: 650px) {
  .task-date-picker {
    max-height: calc(100vh - 36px);
  }
  .date-shortcuts {
    margin-block: 10px;
  }
  .date-calendar td {
    height: 26px;
  }
}
</style>
