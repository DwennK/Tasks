export interface Recurrence {
  anchor_date?: string | null;
  frequency: "daily" | "weekly" | "monthly" | "yearly";
  interval: number;
  weekdays: number[];
  end_date: string | null;
  count: number | null;
  mode: "due_date" | "completion_date";
  month_days?: number[];
  ordinal_weekday?: {
    ordinal: 1 | 2 | 3 | 4 | 5 | -1;
    weekday: number;
  } | null;
  workday?: "first" | "last" | null;
  year_month?: number | null;
  skip_weekends?: boolean;
}
export interface ChecklistItem {
  id: string;
  text: string;
  completed: boolean;
}
export interface Attachment {
  id: string;
  task_id: string;
  name: string;
  mime_type: string;
  size: number;
  created_at: string;
}
export interface TaskComment {
  version: number;
  id: string;
  task_id: string;
  body: string;
  attachment_ids: string[];
  author: string;
  created_at: string;
  updated_at: string;
}
export interface Task {
  progress: number;
  progress_mode: "manual" | "checklist";
  is_note: boolean;
  id: string;
  title: string;
  notes: string;
  notes_text: string;
  notes_format: "plain" | "markdown" | "html";
  list_id: string;
  parent_id: string | null;
  due_date: string | null;
  due_time: string | null;
  start_date: string | null;
  start_time: string | null;
  duration_minutes: number | null;
  timezone: string;
  reminder_at: string | null;
  reminders: string[];
  recurrence: Recurrence | null;
  recurrence_next_id: string | null;
  tag_ids: string[];
  linked_task_ids: string[];
  attachments: Attachment[];
  checklist: ChecklistItem[];
  comments: TaskComment[];
  pinned: boolean;
  wont_do_at: string | null;
  section_id: string | null;
  priority: number;
  position: number;
  completed_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
  version: number;
}
export interface ViewPreferences {
  view_mode: "list" | "board" | "timeline";
  group_by: "none" | "section" | "date" | "priority" | "list" | "tag";
  sort_by: "manual" | "due_date" | "priority" | "title" | "created_at";
  sort_order: "asc" | "desc";
}
export interface TaskList extends ViewPreferences {
  pinned: boolean;
  icon: string;
  list_type: "task" | "note";
  show_in_smart_lists: boolean;
  id: string;
  name: string;
  folder_id: string | null;
  color: string;
  position: number;
  archived_at: string | null;
  created_at: string;
  updated_at: string;
}
export interface Folder {
  pinned: boolean;
  icon: string;
  id: string;
  name: string;
  parent_id: string | null;
  position: number;
  created_at: string;
  updated_at: string;
}
export interface Tag {
  id: string;
  name: string;
  color: string;
  position: number;
  created_at: string;
  updated_at: string;
}
export interface Section {
  id: string;
  list_id: string;
  name: string;
  position: number;
  created_at: string;
  updated_at: string;
}
export interface TaskTemplate {
  id: string;
  name: string;
  task: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}
export interface SavedFilter {
  id: string;
  name: string;
  list_ids: string[];
  tag_ids: string[];
  priorities: number[];
  date: "all" | "today" | "tomorrow" | "week" | "overdue" | "undated";
  match: "all" | "any";
}
export interface Snapshot {
  tasks: Task[];
  lists: TaskList[];
  folders: Folder[];
  tags: Tag[];
  sections: Section[];
  templates: TaskTemplate[];
  settings: Record<string, unknown>;
  revision: number;
}
