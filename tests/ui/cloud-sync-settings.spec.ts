import { expect, test, type Page } from "@playwright/test";
import { settingsCategory, test as appTest } from "./helpers";

// Component fixtures replace only the Tauri boundary. They validate UI state,
// never OAuth authorization, cloud storage, or the sync merge implementation.

async function mountCloudSettings(
  page: Page,
  native = true,
  published = false,
) {
  // Chromium treats this fulfilled HTML document as a synthetic response.
  // Allow its Vite HMR connection only within this temporary local test origin.
  const origin = new URL(test.info().project.use.baseURL!).origin;
  await page.context().grantPermissions(["local-network-access"], { origin });
  await page.route(
    "**/node_modules/.vite/deps/@tauri-apps_api_core.js*",
    (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `export function isTauri(){return ${native}}; export function invoke(command,args){return window.__cloudInvoke(command,args)}`,
      }),
  );
  await page.route(
    "**/node_modules/.vite/deps/@tauri-apps_api_event.js*",
    (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `export async function listen(name,fn){window.__cloudListeners.push(fn);return()=>{window.__cloudListeners=window.__cloudListeners.filter(item=>item!==fn)}}`,
      }),
  );
  await page.route("**/__cloud_settings_fixture__", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><html><head><meta charset="utf-8"></head><body><div id="fixture"></div><script>
    window.__cloudListeners=[];
    window.__cloudCalls=[];
    window.__cloudState={provider:null,configured:false,connected:false,busy:false,autoSync:false,lastSyncAt:null,lastError:null,redirectUri:'http://127.0.0.1:49764/oauth/callback',clientId:'',hasClientSecret:false,supported:true};
    if(${published}) Object.assign(window.__cloudState,{provider:'google_drive',configured:true,clientId:'published-google',availableProviders:{google_drive:true,dropbox:true}});
    window.__cloudConflicts=[{kind:'task',id:'conflicting-task',field:'title',label:'Launch plan',winner_id:'mac-one:4',versions:[{id:'mac-one:4',device_id:'mac-one',clock:4,value:'Launch Monday',truncated:false},{id:'mac-two:5',device_id:'mac-two',clock:5,value:'Launch Tuesday',truncated:false}]}];
    window.__cloudInvoke=async(command,args)=>{
      window.__cloudCalls.push({command,args});
      if(command==='cloud_sync_status')return structuredClone(window.__cloudState);
      if(command==='cloud_sync_configure'){
        if(window.__failCloudConfig)throw new Error('OAuth configuration denied');
        const old=window.__cloudState;
        const disconnected=old.provider!==args.provider||old.clientId!==args.clientId||!!args.clientSecret||args.clearClientSecret;
        Object.assign(old,{provider:args.provider,clientId:args.clientId||(args.provider==='google_drive'?'published-google':'published-dropbox'),autoSync:args.autoSync,configured:true,connected:disconnected?false:old.connected,hasClientSecret:args.clearClientSecret?false:!!args.clientSecret||old.hasClientSecret});
      } else if(command==='cloud_sync_connect')window.__cloudState.connected=true;
      else if(command==='cloud_sync_disconnect')window.__cloudState.connected=false;
      else if(command==='cloud_sync_run')window.__cloudState.lastSyncAt='2026-10-29T12:00:00Z';
      else if(command==='cloud_sync_cancel')window.__cloudState.busy=false;
      else if(command==='dispatch'){
        if(args.action==='sync.status')return {device_id:'mac-one',pending_conflicts:window.__cloudConflicts.length};
        if(args.action==='sync.conflicts')return {items:structuredClone(window.__cloudConflicts)};
        if(args.action==='sync.resolve'){if(${published}) Object.assign(window.__cloudState,{provider:'google_drive',configured:true,clientId:'published-google',availableProviders:{google_drive:true,dropbox:true}});
    window.__cloudConflicts=[];return {ok:true};}
      }
      return structuredClone(window.__cloudState);
    };
    </script><script type="module">
      import {createApp,h} from '/node_modules/.vite/deps/vue.js';
      import CloudSyncSettings from '/src/components/CloudSyncSettings.vue';
      import '/src/style.css';
      createApp({render:()=>h(CloudSyncSettings)}).mount('#fixture');
    </script><style>body{background:var(--surface);overflow:auto}#fixture{max-width:650px;margin:0 auto;padding:24px}button{cursor:pointer}@media(max-width:600px){#fixture{padding:16px}}</style></body></html>`,
    }),
  );
  await page.goto("/__cloud_settings_fixture__");
}

test("cloud settings retain rejected configuration, confirm provider changes and resolve conflicts", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await mountCloudSettings(page);
  await expect(page.getByText("Not configured", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Connect Google Drive", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "open Advanced setup" }).click();
  await page
    .getByLabel("Google Desktop client ID")
    .fill("test-client.apps.googleusercontent.com");
  await page.getByLabel("Google client secret").fill("test-only-client-secret");
  await page
    .getByLabel("Synchronize automatically while Tasks is running")
    .check();
  await page.evaluate(() => {
    (window as any).__failCloudConfig = true;
  });
  await page
    .getByRole("button", { name: "Save Configuration", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "OAuth configuration denied",
  );
  await expect(page.getByLabel("Google Desktop client ID")).toHaveValue(
    "test-client.apps.googleusercontent.com",
  );
  await expect(page.getByLabel("Google client secret")).toHaveValue(
    "test-only-client-secret",
  );
  await page.evaluate(() => {
    (window as any).__failCloudConfig = false;
  });
  await page
    .getByRole("button", { name: "Save Configuration", exact: true })
    .click();
  await expect(
    page.getByText("Ready to connect", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Google client secret")).toHaveValue("");
  await page
    .getByRole("button", { name: "Connect Google Drive", exact: true })
    .click();
  await expect(
    page.getByText("Connected to Google Drive", { exact: true }),
  ).toBeVisible();
  await page
    .getByLabel("Synchronize automatically while Tasks is running")
    .uncheck();
  await page
    .getByRole("button", { name: "Save Configuration", exact: true })
    .click();
  await expect(
    page.getByText("Configuration saved.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Connected to Google Drive", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("Cloud sync provider").selectOption("dropbox");
  await page.getByLabel("Dropbox app key").fill("test-dropbox-key");
  await page
    .getByRole("button", { name: "Save Configuration", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Change the connected configuration?",
  );
  expect(await page.evaluate(() => (window as any).__cloudState.provider)).toBe(
    "google_drive",
  );
  await page
    .getByRole("button", { name: "Save and Disconnect", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Connect Dropbox", exact: true })
    .click();
  await expect(
    page.getByText("Connected to Dropbox", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Synchronize Now", exact: true })
    .click();
  await expect(
    page.getByText("Synchronization finished.", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("region", { name: "Sync conflicts" })
    .getByRole("button", { name: "Keep this version" })
    .nth(1)
    .click();
  await expect(
    page.getByText("No conflicting changes to review."),
  ).toBeVisible();
  const resolution = await page.evaluate(() =>
    (window as any).__cloudCalls.findLast(
      (call: any) =>
        call.command === "dispatch" && call.args.action === "sync.resolve",
    ),
  );
  expect(resolution.args.input).toEqual({
    kind: "task",
    id: "conflicting-task",
    field: "title",
    version_id: "mac-two:5",
  });
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Your local tasks and the data already stored in your cloud account are kept.",
  );
  expect(
    await page.evaluate(() => (window as any).__cloudState.connected),
  ).toBe(true);
  await page
    .getByRole("alert")
    .getByRole("button", { name: "Disconnect", exact: true })
    .click();
  await expect(
    page.getByText("Ready to connect", { exact: true }),
  ).toBeVisible();
  await page.locator(".sync-advanced > summary").click();
  await page.screenshot({
    path: "output/playwright/cloud-sync-desktop.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole("button", { name: "Connect Dropbox", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "output/playwright/cloud-sync-mobile.png",
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

test("browser cloud settings clearly require the desktop app", async ({
  page,
}) => {
  await mountCloudSettings(page, false);
  await expect(
    page.getByText("Desktop app required", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Connect Google Drive", exact: true }),
  ).toBeDisabled();
  await page.locator(".sync-advanced > summary").click();
  await expect(page.getByLabel("Google Desktop client ID")).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__cloudCalls)).toHaveLength(
    0,
  );
});

test("published OAuth configuration lets users connect and select providers without entering credentials", async ({
  page,
}) => {
  await mountCloudSettings(page, true, true);
  await expect(
    page.getByRole("button", { name: "Connect Google Drive", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("Google Desktop client ID")).not.toBeVisible();
  await page
    .getByRole("button", { name: "Connect Google Drive", exact: true })
    .click();
  await page
    .getByRole("group", { name: "Account provider" })
    .getByRole("button", { name: "Dropbox", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Change the connected configuration?",
  );
  await page
    .getByRole("alert")
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  await expect(
    page
      .getByRole("group", { name: "Account provider" })
      .getByRole("button", { name: "Google Drive", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page
    .getByRole("group", { name: "Account provider" })
    .getByRole("button", { name: "Dropbox", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Save and Disconnect", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Connect Dropbox", exact: true }),
  ).toBeEnabled();
  const configured = await page.evaluate(() =>
    (window as any).__cloudCalls.findLast(
      (call: any) => call.command === "cloud_sync_configure",
    ),
  );
  expect(configured.args.clientId).toBe("");
  expect(configured.args.provider).toBe("dropbox");
  await page
    .getByRole("button", { name: "Connect Dropbox", exact: true })
    .click();
  await expect(
    page.getByText("Connected to Dropbox", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Cloud synchronization" }),
  ).toHaveCount(1);
});

appTest("mocked native cloud status events preserve drafts and redact rejected credentials", async ({ page }) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await mountCloudSettings(page, true, true);
  await expect(page.getByText("Ready to connect", { exact: true })).toBeVisible();
  await page.locator(".sync-advanced > summary").click();
  const clientId = page.getByLabel("Google Desktop client ID", { exact: true });
  const secret = page.getByLabel("Google client secret", { exact: true });
  await clientId.fill("unsaved-client.apps.googleusercontent.com");
  await secret.fill("test-secret-not-for-logs");
  await page.getByLabel("Synchronize automatically while Tasks is running").check();
  await page.evaluate(() => {
    const fixture = window as any;
    Object.assign(fixture.__cloudState, {
      lastSyncAt: "2026-10-29T14:00:00Z",
      autoSync: false,
    });
    for (const listener of fixture.__cloudListeners)
      listener({ payload: structuredClone(fixture.__cloudState) });
    const originalInvoke = fixture.__cloudInvoke;
    fixture.__cloudInvoke = async (command: string, args: unknown) => {
      if (command === "cloud_sync_configure") {
        fixture.__cloudCalls.push({ command, args });
        throw new Error(
          "Rejected test-secret-not-for-logs access_token=test-access-token refresh_token=test-refresh-token client_secret=test-extra-secret authorization_code=test-auth-code Bearer test-bearer-token",
        );
      }
      return originalInvoke(command, args);
    };
  });
  await expect(clientId).toHaveValue("unsaved-client.apps.googleusercontent.com");
  await expect(secret).toHaveValue("test-secret-not-for-logs");
  await expect(page.getByLabel("Synchronize automatically while Tasks is running")).toBeChecked();
  await expect(page.getByText("Unsaved configuration", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Save Configuration", exact: true }).click();
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("Rejected [redacted]");
  for (const value of ["test-secret-not-for-logs", "test-access-token", "test-refresh-token", "test-extra-secret", "test-auth-code", "test-bearer-token"])
    await expect(alert).not.toContainText(value);
  await expect(alert).toContainText("access_token=[redacted]");
  await expect(alert).toContainText("Bearer [redacted]");
  await expect(clientId).toHaveValue("unsaved-client.apps.googleusercontent.com");
  await expect(secret).toHaveValue("test-secret-not-for-logs");
  await expect(page.getByRole("button", { name: "Save Configuration", exact: true })).toBeEnabled();
});

appTest("mocked native sign-in can be cancelled without claiming an account connection", async ({ page }) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await mountCloudSettings(page, true, true);
  await expect(page.getByText("Ready to connect", { exact: true })).toBeVisible();
  await page.evaluate(() => {
    const fixture = window as any;
    const originalInvoke = fixture.__cloudInvoke;
    let finishConnect: ((value: unknown) => void) | undefined;
    fixture.__cloudInvoke = async (command: string, args: unknown) => {
      if (command === "cloud_sync_connect") {
        fixture.__cloudCalls.push({ command, args });
        fixture.__cloudState.busy = true;
        for (const listener of fixture.__cloudListeners)
          listener({ payload: structuredClone(fixture.__cloudState) });
        return new Promise(resolve => { finishConnect = resolve; });
      }
      if (command === "cloud_sync_cancel") {
        fixture.__cloudCalls.push({ command, args });
        fixture.__cloudState.busy = false;
        const state = structuredClone(fixture.__cloudState);
        finishConnect?.(state);
        for (const listener of fixture.__cloudListeners)
          listener({ payload: structuredClone(state) });
        return state;
      }
      return originalInvoke(command, args);
    };
  });
  const connect = page.getByRole("button", { name: "Connect Google Drive", exact: true });
  await connect.click();
  await expect(page.getByText("Waiting for browser sign-in…", { exact: true })).toBeVisible();
  await expect(connect).toBeDisabled();
  await expect(page.getByRole("group", { name: "Account provider" }).getByRole("button", { name: "Dropbox", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Cancel operation", exact: true }).click();
  await expect(page.getByText("Ready to connect", { exact: true })).toBeVisible();
  await expect(connect).toBeEnabled();
  await expect(page.getByText("Cancellation requested. Any change already applied is kept.", { exact: true })).toBeVisible();
  await expect(page.getByText("Connected to Google Drive", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Cancel operation", exact: true })).toHaveCount(0);
  const calls = await page.evaluate(() => (window as any).__cloudCalls.map((entry: any) => entry.command));
  expect(calls.filter((command: string) => command === "cloud_sync_connect")).toHaveLength(1);
  expect(calls.filter((command: string) => command === "cloud_sync_cancel")).toHaveLength(1);
  expect(calls).not.toContain("cloud_sync_run");
});

appTest("real browser Settings screen requires the desktop app without invoking native cloud commands", async ({ page }) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  // No fixture, routes, native API stubs, credentials, or cloud requests here.
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
  const settings = await settingsCategory(page, "Cloud Sync");
  const cloud = settings.getByRole("region", { name: "Cloud synchronization", exact: true });
  await expect(cloud.getByText("Desktop app required", { exact: true })).toBeVisible();
  await expect(cloud.getByRole("button", { name: "Connect Google Drive", exact: true })).toBeDisabled();
  await expect(cloud.getByRole("button", { name: "Refresh cloud sync status", exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => typeof (window as any).__TAURI_INTERNALS__)).toBe("undefined");
  await cloud.locator(".sync-advanced > summary").click();
  await expect(cloud.getByLabel("Google Desktop client ID", { exact: true })).toBeDisabled();
  await expect(cloud.getByLabel("Google client secret", { exact: true })).toBeDisabled();
  await expect(cloud.getByRole("button", { name: "Save Configuration", exact: true })).toBeDisabled();
  await expect(settings.getByRole("alert")).toHaveCount(0);
  await page.screenshot({ path: "output/playwright/cloud-sync-browser-settings-desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(cloud.getByText("Desktop app required", { exact: true })).toBeVisible();
  await expect(cloud.getByRole("button", { name: "Connect Google Drive", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.screenshot({ path: "output/playwright/cloud-sync-browser-settings-mobile.png" });
  await settings.getByRole("button", { name: "Close settings", exact: true }).click();
  await expect(settings).toBeHidden();
});
