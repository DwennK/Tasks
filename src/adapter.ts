import { invoke, isTauri } from "@tauri-apps/api/core";
export const isNative = isTauri();

export async function dispatch<T = unknown>(
  action: string,
  input: Record<string, unknown> = {},
): Promise<T> {
  if (!isNative) {
    if (
      import.meta.env.DEV &&
      import.meta.env.VITE_TASKS_DEV_BRIDGE === "true"
    ) {
      const response = await fetch("/__tasks", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, input }),
      });
      const result = await response.json();
      if (!response.ok)
        throw new Error(
          result.error || "The local test database is unavailable.",
        );
      return result as T;
    }
    throw new Error(
      "Open Tasks for local database access. The browser preview does not access your data.",
    );
  }
  return invoke<T>("dispatch", { action, input });
}
