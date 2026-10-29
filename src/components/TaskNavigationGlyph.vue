<script setup lang="ts">
import { computed } from "vue";
import { localDate } from "../composables/useTasks";

withDefaults(
  defineProps<{
    kind?:
      | "all"
      | "today"
      | "tomorrow"
      | "upcoming"
      | "inbox"
      | "filters"
      | "sidebar";
    size?: number;
  }>(),
  { size: 19 },
);

// Reuse the application's local-day invalidation, including wake and midnight.
const today = computed(() => localDate());
const dayNumber = computed(() => String(Number(today.value.slice(-2))));
const weekday = computed(() =>
  new Date(`${today.value}T12:00:00`)
    .toLocaleDateString("en-US", { weekday: "short" })
    .slice(0, 2),
);
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    :width="size"
    :height="size"
    viewBox="0 0 20 20"
    fill="none"
    stroke="currentColor"
    stroke-width="1.45"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
    :data-task-glyph="kind"
    :data-local-day="
      kind === 'today' || kind === 'upcoming' ? today : undefined
    "
  >
    <template v-if="kind === 'today' || kind === 'upcoming'">
      <rect x="3" y="2.6" width="14" height="14" rx="3.3" />
      <path d="M3.3 6.2h13.4" />
      <text
        x="10"
        y="13.6"
        fill="currentColor"
        stroke="none"
        text-anchor="middle"
        font-family="-apple-system, BlinkMacSystemFont, sans-serif"
        font-size="7.2"
        :font-weight="kind === 'today' ? 600 : 500"
      >
        {{ kind === "today" ? dayNumber : weekday }}
      </text>
    </template>
    <template v-else-if="kind === 'inbox'">
      <path
        d="m2.4 10.4 3.1-5a1.7 1.7 0 0 1 1.4-.8h6.2a1.7 1.7 0 0 1 1.4.8l3.1 5v4.5a1.9 1.9 0 0 1-1.9 1.9H4.3a1.9 1.9 0 0 1-1.9-1.9z"
      />
      <path
        d="M2.6 10.4h4.3l.7 1.5a1.3 1.3 0 0 0 1.2.8h2.4a1.3 1.3 0 0 0 1.2-.8l.7-1.5h4.3"
      />
    </template>
    <template v-else-if="kind === 'all'">
      <path d="M6.6 2.5h6.8M4.6 4.6h10.8" />
      <rect x="2.5" y="6.5" width="15" height="10.4" rx="2.8" />
    </template>
    <template v-else-if="kind === 'tomorrow'">
      <path
        d="M3 17.2h14M6.3 17.1a5.1 5.1 0 1 1 7.4 0M10 7.5V1.8m-3.2 3.3L10 1.8l3.2 3.3M3.4 8.1 2.3 7M2.6 12.1H1.4M16.6 8.1 17.7 7m-.3 5.1h1.2"
      />
    </template>
    <template v-else-if="kind === 'filters'">
      <path d="M3 4.4h14M3 9.8h4.8M3 15.3h6.5" />
      <path
        d="M10.5 8.5h7l-2.7 3.2v5.1l-1.6-.9v-4.2z"
        fill="currentColor"
        stroke="none"
      />
    </template>
    <template v-else-if="kind === 'sidebar'">
      <path d="M3 4.7h14M3 10h7.8M3 15.3h14m0-7.8L14.5 10l2.5 2.5" />
    </template>
  </svg>
</template>
