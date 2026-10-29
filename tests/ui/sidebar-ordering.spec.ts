import { expect, type Locator, type Page } from "@playwright/test";
import { test, call, resetSettings, snapshot, unique } from "./helpers";
import type { Folder, TaskList } from "../../src/types";

type Fixtures = { folders: Folder[]; lists: TaskList[] };
const container = (page: Page, item: Folder | TaskList) =>
  page.getByRole("treeitem", { name: item.name, exact: true });

async function folder(
  page: Page,
  fixture: Fixtures,
  label: string,
  parent?: Folder,
  pinned = false,
) {
  const created = await call<Folder>(page.request, "folders.create", {
    name: unique(label),
    parent_id: parent?.id ?? null,
  });
  fixture.folders.push(created);
  if (pinned)
    await call(page.request, "folders.update", {
      id: created.id,
      pinned: true,
    });
  return created;
}
async function list(
  page: Page,
  fixture: Fixtures,
  label: string,
  parent: Folder,
  pinned = false,
) {
  const created = await call<TaskList>(page.request, "lists.create", {
    name: unique(label),
    folder_id: parent.id,
  });
  fixture.lists.push(created);
  if (pinned)
    await call(page.request, "lists.update", { id: created.id, pinned: true });
  return created;
}
async function cleanup(page: Page, fixture: Fixtures) {
  for (const item of fixture.lists)
    await call(page.request, "lists.delete", { id: item.id });
  for (const item of [...fixture.folders].reverse())
    await call(page.request, "folders.delete", { id: item.id });
}
async function dropRelative(
  page: Page,
  source: Folder | TaskList,
  target: Folder | TaskList,
  side: "before" | "after",
) {
  const targetRow = container(page, target);
  const box = (await targetRow.boundingBox())!;
  await container(page, source).dragTo(targetRow, {
    targetPosition: { x: 55, y: side === "before" ? 2 : box.height - 2 },
  });
}
async function renderedOrder(page: Page, ids: string[]) {
  return page
    .getByRole("tree", { name: "Task lists", exact: true })
    .locator("[data-container-id]")
    .evaluateAll(
      (elements, selected) =>
        elements
          .map((element) => element.getAttribute("data-container-id")!)
          .filter((id) => selected.includes(id)),
      ids,
    );
}
async function hoverWhileDragging(
  page: Page,
  source: Locator,
  target: Locator,
  ratio: number,
) {
  await source.scrollIntoViewIfNeeded();
  await target.scrollIntoViewIfNeeded();
  const sourceBox = (await source.boundingBox())!;
  const targetBox = (await target.boundingBox())!;
  const start = { x: sourceBox.x + 55, y: sourceBox.y + sourceBox.height / 2 };
  const end = {
    x: targetBox.x + 55,
    y: targetBox.y + targetBox.height * ratio,
  };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 9, start.y, { steps: 5 });
  await page.mouse.move(end.x, end.y, { steps: 10 });
  await page.mouse.move(end.x + 1, end.y);
  await expect(page.locator(".tree-root-drop")).toContainText(
    "Move to top level",
  );
}

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
});

test("sidebar list drops across folders honor before and after instead of appending", async ({
  page,
}) => {
  const fixture: Fixtures = { folders: [], lists: [] };
  try {
    const source = await folder(page, fixture, "List order source");
    const target = await folder(page, fixture, "List order destination");
    const before = await list(page, fixture, "Cross-folder before", source);
    const after = await list(page, fixture, "Cross-folder after", source);
    const first = await list(page, fixture, "Destination first", target);
    const middle = await list(page, fixture, "Destination middle", target);
    const last = await list(page, fixture, "Destination last", target);
    await page.goto("/");
    await expect(container(page, before)).toBeVisible();
    await dropRelative(page, before, middle, "before");
    await expect
      .poll(async () =>
        (await snapshot(page)).lists
          .filter((item) => item.folder_id === target.id)
          .sort((a, b) => a.position - b.position)
          .map((item) => item.id),
      )
      .toEqual([first.id, before.id, middle.id, last.id]);
    await dropRelative(page, after, middle, "after");
    const expected = [first.id, before.id, middle.id, after.id, last.id];
    await expect
      .poll(async () =>
        (await snapshot(page)).lists
          .filter((item) => item.folder_id === target.id)
          .sort((a, b) => a.position - b.position)
          .map((item) => item.id),
      )
      .toEqual(expected);
    await expect.poll(() => renderedOrder(page, expected)).toEqual(expected);
    await page.reload();
    await expect(container(page, after)).toBeVisible();
    expect(await renderedOrder(page, expected)).toEqual(expected);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, fixture);
  }
});

test("sidebar nested folder drops across parents retain the requested relative positions", async ({
  page,
}) => {
  const fixture: Fixtures = { folders: [], lists: [] };
  try {
    const source = await folder(page, fixture, "Folder order source");
    const target = await folder(page, fixture, "Folder order destination");
    const before = await folder(page, fixture, "Nested before", source);
    const after = await folder(page, fixture, "Nested after", source);
    const first = await folder(page, fixture, "Nested first", target);
    const middle = await folder(page, fixture, "Nested middle", target);
    const last = await folder(page, fixture, "Nested last", target);
    await page.goto("/");
    await expect(container(page, before)).toBeVisible();
    await dropRelative(page, before, middle, "before");
    await expect
      .poll(async () =>
        (await snapshot(page)).folders
          .filter((item) => item.parent_id === target.id)
          .sort((a, b) => a.position - b.position)
          .map((item) => item.id),
      )
      .toEqual([first.id, before.id, middle.id, last.id]);
    await dropRelative(page, after, middle, "after");
    const expected = [first.id, before.id, middle.id, after.id, last.id];
    await expect
      .poll(async () =>
        (await snapshot(page)).folders
          .filter((item) => item.parent_id === target.id)
          .sort((a, b) => a.position - b.position)
          .map((item) => item.id),
      )
      .toEqual(expected);
    await expect.poll(() => renderedOrder(page, expected)).toEqual(expected);
    await page.reload();
    await expect(container(page, after)).toBeVisible();
    expect(await renderedOrder(page, expected)).toEqual(expected);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, fixture);
  }
});

test("sidebar rejects misleading cross-kind and pin-boundary indicators while allowing folder-center nesting", async ({
  page,
}) => {
  const fixture: Fixtures = { folders: [], lists: [] };
  try {
    const source = await folder(page, fixture, "Rejected drop source");
    const target = await folder(page, fixture, "Rejected drop destination");
    const movingFolder = await folder(
      page,
      fixture,
      "Moving ordinary folder",
      source,
    );
    const movingList = await list(
      page,
      fixture,
      "Moving ordinary list",
      source,
    );
    const targetFolder = await folder(
      page,
      fixture,
      "Other kind folder",
      target,
    );
    const targetList = await list(page, fixture, "Other kind list", target);
    const pinnedFolder = await folder(
      page,
      fixture,
      "Pinned destination folder",
      target,
      true,
    );
    const pinnedList = await list(
      page,
      fixture,
      "Pinned destination list",
      target,
      true,
    );
    await page.goto("/");
    await expect(container(page, movingList)).toBeVisible();
    const original = await snapshot(page);
    const cases = [
      { source: movingList, target: targetFolder, ratio: 0.05 },
      { source: movingFolder, target: targetList, ratio: 0.5 },
      { source: movingList, target: pinnedList, ratio: 0.95 },
      { source: movingFolder, target: pinnedFolder, ratio: 0.05 },
    ];
    for (const item of cases) {
      try {
        await hoverWhileDragging(
          page,
          container(page, item.source),
          container(page, item.target),
          item.ratio,
        );
        await expect(
          page.locator(
            ".tree-row.drop-before, .tree-row.drop-after, .tree-row.drop-inside",
          ),
        ).toHaveCount(0);
      } finally {
        await page.mouse.up();
      }
      await page.waitForLoadState("networkidle");
      const unchanged = await snapshot(page);
      expect(unchanged.revision).toBe(original.revision);
      expect(
        unchanged.lists.find((item) => item.id === movingList.id)?.folder_id,
      ).toBe(source.id);
      expect(
        unchanged.folders.find((item) => item.id === movingFolder.id)
          ?.parent_id,
      ).toBe(source.id);
    }
    try {
      await hoverWhileDragging(
        page,
        container(page, movingList),
        container(page, targetFolder),
        0.5,
      );
      await expect(container(page, targetFolder)).toHaveClass(/drop-inside/);
    } finally {
      await page.mouse.up();
    }
    await expect
      .poll(
        async () =>
          (await snapshot(page)).lists.find((item) => item.id === movingList.id)
            ?.folder_id,
      )
      .toBe(targetFolder.id);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await page.mouse.up();
    await cleanup(page, fixture);
  }
});
