import { expect, test, type Page } from "@playwright/test";
import { test as appTest, settingsCategory } from "./helpers";

// Actual component, with only native IPC/events replaced. This is not an
// end-to-end update installation and never touches an installed app.
async function mount(page: Page, native = true) {
  const origin = new URL(test.info().project.use.baseURL!).origin;
  await page.context().grantPermissions(["local-network-access"], { origin });
  await page.route(
    "**/node_modules/.vite/deps/@tauri-apps_api_core.js*",
    (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `export function isTauri(){return ${native}};export function invoke(c,a){return window.__updateInvoke(c,a)}`,
      }),
  );
  await page.route(
    "**/node_modules/.vite/deps/@tauri-apps_api_event.js*",
    (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `export async function listen(n,f){window.__updateListener=f;return()=>{window.__updateListener=null}}`,
      }),
  );
  await page.route("**/__updates_fixture__", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: String.raw`<!doctype html><html><head><meta charset="utf-8"></head><body><div id="fixture"></div><script>
  window.__calls=[];
  window.__status={currentVersion:'0.2.0',phase:'idle',version:null,notes:null,downloaded:0,total:null,error:null,checkedAt:null,supported:true,disabledReason:null,automatic:true};
  window.__emit=(patch)=>{Object.assign(window.__status,patch);window.__updateListener?.({payload:structuredClone(window.__status)})};
  window.__updateInvoke=async(c,a)=>{
    window.__calls.push(c);
    if(c==='update_status')return structuredClone(window.__status);
    if(c==='update_set_automatic'){if(window.__reject)throw Error('Could not save preference');window.__status.automatic=a.enabled;}
    if(c==='check_for_updates'){
      if(window.__reject){window.__emit({phase:'error',error:'Could not check for updates. Offline.'});}
      else window.__emit({phase:'available',error:null,version:'0.3.0',notes:'New release\n<img src=x onerror=alert(1)>',checkedAt:'2026-10-29T09:00:00Z'});
    }
    if(c==='download_update'){
      window.__emit({phase:'downloading',downloaded:50,total:100});
      await new Promise(resolve=>window.__finishDownload=resolve);
      window.__emit(window.__badSignature?{phase:'error',error:'The update could not be downloaded or verified.'}:{phase:'ready',downloaded:100});
    }
    if(c==='install_update'){window.__emit({phase:'saving'});return null;}
    return structuredClone(window.__status);
  };
  </script><script type="module">
  import {createApp,h} from '/node_modules/.vite/deps/vue.js';
  import UpdateSettings from '/src/components/UpdateSettings.vue';
  import '/src/style.css';
  createApp({render:()=>h(UpdateSettings)}).mount('#fixture');
  </script><style>body{background:var(--surface);overflow:auto}#fixture{max-width:650px;margin:0 auto;padding:24px}</style></body></html>`,
    }),
  );
  await page.goto("/__updates_fixture__");
}

test("updates require verified download and a separate install action", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await mount(page);
  await page
    .getByRole("button", { name: "Check for updates", exact: true })
    .click();
  await expect(page.getByRole("status")).toHaveText("Tasks 0.3.0 is available");
  await expect(page.locator(".release-notes img")).toHaveCount(0);
  await page.getByRole("button", { name: "Download update" }).click();
  await expect(page.getByRole("progressbar")).toHaveAttribute("value", "50");
  await expect(
    page.getByRole("button", { name: "Install and restart" }),
  ).toHaveCount(0);
  await page.evaluate(() => (window as any).__finishDownload());
  await expect(page.getByRole("status")).toHaveText("Ready to install");
  expect(
    await page.evaluate(
      () =>
        (window as any).__calls.filter((x: string) => x === "install_update")
          .length,
    ),
  ).toBe(0);
  await page.screenshot({
    path: "output/playwright/updates-ready-desktop.png",
  });
  await page.getByRole("button", { name: "Install and restart" }).click();
  await expect(page.getByRole("status")).toHaveText("Saving your changes…");
  await expect(
    page.getByRole("button", { name: "Check for updates", exact: true }),
  ).toBeDisabled();
  expect(errors).toEqual([]);
});

test("update verification failures never offer installation and can be retried", async ({
  page,
}) => {
  await mount(page);
  await page
    .getByRole("button", { name: "Check for updates", exact: true })
    .click();
  await page.evaluate(() => {
    (window as any).__badSignature = true;
  });
  await page.getByRole("button", { name: "Download update" }).click();
  await page.evaluate(() => (window as any).__finishDownload());
  await expect(page.getByRole("alert")).toContainText(
    "could not be downloaded or verified",
  );
  await expect(
    page.getByRole("button", { name: "Install and restart" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Check for updates", exact: true })
    .click();
  await expect(page.getByRole("status")).toHaveText("Tasks 0.3.0 is available");
});

test("rejected update preferences retain the saved choice and offline checks remain recoverable", async ({
  page,
}) => {
  await mount(page);
  await page.evaluate(() => {
    (window as any).__reject = true;
  });
  await page.getByLabel("Automatically check for updates").click();
  await expect(
    page.getByLabel("Automatically check for updates"),
  ).toBeChecked();
  await expect(page.getByRole("alert")).toContainText(
    "Could not save preference",
  );
  await page
    .getByRole("button", { name: "Check for updates", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("Offline");
  await page.evaluate(() => {
    (window as any).__reject = false;
  });
  await page.getByLabel("Automatically check for updates").uncheck();
  await expect(
    page.getByLabel("Automatically check for updates"),
  ).not.toBeChecked();
});

test("browser preview cannot invoke update commands and fits mobile", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mount(page, false);
  await expect(
    page.getByText("Open the desktop app to check for and install updates."),
  ).toBeVisible();
  await expect(
    page.getByLabel("Automatically check for updates"),
  ).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__calls)).toEqual([]);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: "output/playwright/updates-mobile.png" });
});

appTest(
  "Updates is reachable through the real desktop Settings navigation",
  async ({ page }) => {
    await page.goto("/");
    await settingsCategory(page, "Updates");
    await expect(
      page.getByRole("region", { name: "Application updates" }),
    ).toBeVisible();
    await expect(
      page.getByText("Open the desktop app to check for and install updates."),
    ).toBeVisible();
    await page.screenshot({
      path: "output/playwright/updates-settings-desktop.png",
    });
  },
);
