# Personal cloud synchronization

Tasks can synchronize personal data between Macs through one Google Drive or Dropbox account at a time. The local database stays available offline. This is personal synchronization; it does not provide shared workspaces, invitations or assignments.

## Publisher configuration

The publisher registers **one OAuth application per provider**, then distributes its public client configuration with Tasks. End users authorize their own account in their system browser; they do not need to register a developer application. Google Desktop clients are public clients: a bundled Desktop client secret is not a confidential server credential. Access and refresh tokens are per user and stay in macOS Keychain.

The native build accepts these environment variables at compile time:

- `TASKS_GOOGLE_CLIENT_ID`: Google OAuth **Desktop app** client ID.
- `TASKS_GOOGLE_CLIENT_SECRET`: optional secret generated for that Desktop client.
- `TASKS_DROPBOX_APP_KEY`: Dropbox scoped **App folder** app key.

With a published provider configured, Settings → Cloud Sync offers Connect immediately. When both providers are included, users can choose one there. Switching the connected provider asks for confirmation, disconnects the previous local session and keeps existing local and cloud data.

No production OAuth IDs are committed to this repository. A build without these values displays an unconfigured state. Advanced app configuration supports a publisher-supplied client ID or a personal developer app for testing. A successful local build does not establish that a real OAuth connection or cloud transfer has been verified.

### Google Drive registration

1. Create or select a Google Cloud project and [enable the Drive API](https://console.cloud.google.com/apis/library/drive.googleapis.com).
2. Configure the OAuth consent screen, including test users while the app is in testing.
3. Create a [Desktop app OAuth client](https://console.cloud.google.com/auth/clients). Use its ID for all distributed installations of the same app.
4. Request only `https://www.googleapis.com/auth/drive.appdata` for synchronization. This accesses the application's hidden data folder, not the user's ordinary Drive files.
5. Complete Google's applicable publication and verification requirements before a public release. Test-mode access and refresh-token policies are not a production distribution strategy.

References: [Google Desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app), [Drive application data](https://developers.google.com/workspace/drive/api/guides/appdata).

### Dropbox registration

1. Create a scoped app with **App folder** access in the [Dropbox App Console](https://www.dropbox.com/developers/apps).
2. Enable `files.metadata.read`, `files.content.read` and `files.content.write`.
3. Register the exact redirect URI `http://127.0.0.1:49764/oauth/callback`.
4. Distribute its public app key with Tasks and complete Dropbox's applicable production-access requirements.

Tasks uses a system-browser OAuth flow with PKCE. It does not ask users to paste a generated access token. See [Dropbox's OAuth guide](https://developers.dropbox.com/oauth-guide).

## Using synchronization

Open Settings → Cloud Sync, choose the provider when offered, and click Connect. Authorize the same account on each Mac. After authorization, Synchronize Now exchanges changes. Automatic synchronization is optional; it sends local changes and periodically checks for remote updates while Tasks is running. Updates use revision checks, changed-file downloads, unchanged-upload suppression and backoff after provider errors.

The panel reports native connection state, the most recent successful synchronization and errors. Changing a configuration does not silently authorize an account. Failed configuration saves preserve entered values for retry. Client secrets are never returned by the status command.

Concurrent changes to different synchronized fields are merged. For conflicting changes to the same field, the panel preserves and displays the alternative versions. “Currently applied” identifies the deterministic value currently in use. Choose a version, then synchronize that choice to the other Macs. A shortened preview does not truncate the value restored by the selection. A deletion conflict explicitly offers applying deletion or keeping the item.

Disconnecting removes the local authorization and stops synchronization on that Mac. It does not delete the local database or provider files. Switching providers does not automatically migrate or delete the old provider's files.

## Limits and costs

Current transport limits are 32 device files, 64 MiB per device packet and 128 MiB of changed packets per synchronization pass. Exceeding a limit stops the operation with an error; it is not a request to delete user data. Attachments are included in synchronized packets and count toward these limits. Saved filters (`saved_filters`) and summary templates (`summary_templates`) are portable and synchronize between Macs. Machine settings, OAuth credentials and local activity history remain local.

Synchronization is opt-in. While inactive, normal task editing does not create synchronization replicas or duplicate attachment BLOBs. Development builds and custom-database sessions disable native cloud synchronization to keep QA data isolated from the user's cloud account; Settings displays that reason explicitly.

Do not describe large-scale Drive synchronization as unconditionally free. Google's [Drive API limits and billing thresholds](https://developers.google.com/workspace/drive/api/guides/limits) changed in 2026; current documentation distinguishes project/user quotas, daily billing thresholds and egress. Check the applicable project quotas and current billing terms before public distribution. A user's storage plan and capacity are separate from API usage. Dropbox also applies its own account capacity, app-access requirements and API limits.

The browser test fixture validates settings interactions and command contracts with explicit native-call stubs. Real provider authorization, token refresh and transfer between two Macs require configured OAuth applications and consenting test accounts; those are separate release checks.
