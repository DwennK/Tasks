import { expect, type Locator } from "@playwright/test";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import type { Task, TaskList } from "../../src/types";
import { call, resetSettings, snapshot, test, unique } from "./helpers";

function installedDragScript() {
  const lock = readFileSync(resolve("Cargo.lock"), "utf8");
  const version = lock.match(
    /\[\[package\]\]\nname = "tauri"\nversion = "([^"]+)"/,
  )?.[1];
  expect(
    version,
    "Rust SDK version comes from Cargo.lock, not the npm API",
  ).toBe("2.12.5");
  const registry = join(homedir(), ".cargo", "registry", "src");
  const path = readdirSync(registry)
    .map((source) =>
      join(registry, source, `tauri-${version}`, "src/window/scripts/drag.js"),
    )
    .find(existsSync);
  expect(
    path,
    "Use the installed SDK; do not replace its drag algorithm",
  ).toBeTruthy();
  const script = readFileSync(path!, "utf8");
  expect(script).toContain("__TEMPLATE_os_name__");
  return {
    version,
    path: path!,
    sha256: createHash("sha256").update(script).digest("hex"),
    content: script.replace("__TEMPLATE_os_name__", JSON.stringify("macos")),
  };
}

test("the locked Tauri macOS drag handler accepts title-bar blanks and excludes controls, popovers and resizers", async ({
  page,
}, info) => {
  await resetSettings(page);
  const sdk = installedDragScript();
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Drag hit testing"),
  });
  const checks: Record<string, unknown>[] = [];
  try {
    const task = await call<Task>(page.request, "tasks.create", {
      title: unique("Editable drag fixture"),
      list_id: list.id,
      notes: "Notes must remain editable, never a window drag surface.",
    });
    const calls = () =>
      page.evaluate(() => [...(window as any).__dragCalls] as string[]);
    const probe = async (
      label: string,
      x: number,
      y: number,
      drag: boolean,
    ) => {
      const before = await calls();
      const hit = await page.evaluate(
        ({ x, y }) => {
          const element = document.elementFromPoint(x, y);
          return {
            tag: element?.tagName,
            class: element?.getAttribute("class"),
            role: element?.getAttribute("role"),
          };
        },
        { x, y },
      );
      await page.mouse.click(x, y, { clickCount: 1 });
      expect(await calls(), `${label}: ${JSON.stringify(hit)}`).toEqual(
        drag ? [...before, "plugin:window|start_dragging"] : before,
      );
      checks.push({ label, x, y, hit, drag });
    };
    const clickControl = async (label: string, locator: Locator) => {
      const before = await calls();
      await locator.click();
      expect(await calls(), label).toEqual(before);
      checks.push({ label, drag: false });
    };
    for (const [style, font] of [
      ["card", "normal"],
      ["flat", "extra-large"],
    ] as const) {
      await call(page.request, "settings.set", {
        key: "interface_style",
        value: style,
      });
      await call(page.request, "settings.set", {
        key: "font_size",
        value: font,
      });
      await page.goto("/");
      await page
        .getByRole("treeitem", { name: list.name, exact: true })
        .click();
      await expect(
        page.getByText("Local preview", { exact: true }),
      ).toBeVisible();
      await expect(
        page.getByText("Loading tasks…", { exact: true }),
      ).toBeHidden();
      // App/adapter has already captured isNative=false and keeps using the
      // real Rust HTTP bridge. Only this SDK script sees the invocation spy.
      await page.evaluate(() => {
        (window as any).__dragCalls = [];
        (window as any).__TAURI_INTERNALS__ = {
          invoke: async (command: string) => {
            (window as any).__dragCalls.push(command);
          },
        };
      });
      await page.addScriptTag({ content: sdk.content });
      const empty = (await page.locator(".empty-detail").boundingBox())!;
      const band = (await page.locator(".detail-window-drag").boundingBox())!;
      expect(band).toEqual({
        x: empty.x,
        y: empty.y,
        width: empty.width,
        height: 57,
      });
      expect(band.y).toBe(style === "card" ? 7 : 0);
      await probe(
        `${style}/${font}: empty top-right band`,
        band.x + band.width - 50,
        band.y + 35,
        true,
      );
      await probe(
        `${style}/${font}: below the 57px band`,
        band.x + band.width - 50,
        band.y + 58,
        false,
      );
      if (style === "card") await probe("Card outer frame", 1450, 2, true);
      const heading = (await page.locator(".list-header h1").boundingBox())!;
      await probe(
        `${style}: list heading`,
        heading.x + 10,
        heading.y + heading.height / 2,
        true,
      );

      await clickControl(
        "sort button SVG",
        page
          .getByRole("button", { name: "Sort tasks", exact: true })
          .locator("svg"),
      );
      const sort = page.getByRole("menu", {
        name: "Sort and group tasks",
        exact: true,
      });
      await expect(sort).toBeVisible();
      const caption = (await sort
        .locator(".popover-section-caption")
        .first()
        .boundingBox())!;
      await probe(
        "noninteractive caption inside a popover",
        caption.x + 10,
        caption.y + caption.height / 2,
        false,
      );
      await page.keyboard.press("Escape");
      await clickControl(
        "task title button",
        page.getByRole("button", {
          name: `Open task ${task.title}`,
          exact: true,
        }),
      );
      const detailTitle = page.getByRole("textbox", {
        name: "Task title",
        exact: true,
      });
      await expect(detailTitle).toHaveValue(task.title);
      const toolbar = (await page.locator(".editor-toolbar").boundingBox())!;
      await probe(
        `${style}/${font}: right toolbar blank`,
        toolbar.x + toolbar.width - 100,
        toolbar.y + 15,
        true,
      );
      await clickControl(
        "date button SVG",
        page
          .getByRole("button", { name: "Set due date", exact: true })
          .locator("svg")
          .first(),
      );
      const picker = page.getByRole("dialog", {
        name: "Task date and reminders",
        exact: true,
      });
      await expect(picker).toBeVisible();
      const calendar = (await picker.boundingBox())!;
      // x+2,y+2 lies outside rounded corners and legitimately hits the toolbar.
      await probe(
        "actual calendar background",
        calendar.x + 12,
        calendar.y + 12,
        false,
      );
      await page.keyboard.press("Escape");
      await clickControl(
        "priority button SVG",
        page
          .getByRole("button", { name: "Set priority", exact: true })
          .locator("svg"),
      );
      await expect(
        page.getByRole("menu", { name: "Priority", exact: true }),
      ).toBeVisible();
      await clickControl(
        "priority menu item",
        page.getByRole("menuitem", { name: "No Priority", exact: true }),
      );
      await clickControl("editable detail title", detailTitle);
      await clickControl(
        "editable notes",
        page.getByRole("textbox", { name: "Task notes", exact: true }),
      );

      const resize = page.getByRole("separator", {
        name: "Resize task list",
        exact: true,
      });
      const separator = (await resize.boundingBox())!;
      const beforeResize = await calls();
      const initialWidth = Number(await resize.getAttribute("aria-valuenow"));
      await page.mouse.move(
        separator.x + separator.width / 2,
        separator.y + 25,
      );
      await page.mouse.down();
      await page.mouse.move(
        separator.x + separator.width / 2 + 20,
        separator.y + 25,
        { steps: 3 },
      );
      await page.mouse.up();
      expect(await calls()).toEqual(beforeResize);
      await expect
        .poll(async () => (await snapshot(page)).settings.list_width)
        .toBe(initialWidth + 20);
      checks.push({
        label: `${style}: real pointer resize, not window drag`,
        drag: false,
      });

      await page.keyboard.press("Meta+k");
      const dialog = page.getByRole("dialog", {
        name: "Command Menu",
        exact: true,
      });
      await expect(dialog).toBeVisible();
      const dialogBox = (await dialog.boundingBox())!;
      await probe(
        "foreground modal padding",
        dialogBox.x + 12,
        dialogBox.y + 12,
        false,
      );
      await page.keyboard.press("Escape");
      await expect(dialog).toBeHidden();
      await expect(page.getByRole("alert")).toHaveCount(0);
    }
    expect(
      (await snapshot(page)).tasks.find((item) => item.id === task.id),
    ).toMatchObject({
      title: task.title,
      notes: task.notes,
      completed_at: null,
    });
    await info.attach("tauri-drag-sdk-hit-tests", {
      body: JSON.stringify(
        {
          sdkVersion: sdk.version,
          sdkPath: sdk.path,
          sdkSha256: sdk.sha256,
          checks,
          limitation:
            "Real SDK event routing with an invoke spy; no native window movement is claimed.",
        },
        null,
        2,
      ),
      contentType: "application/json",
    });
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (item) => item.list_id === list.id && !item.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
