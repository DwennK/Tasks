# Releases and updates

Tasks is distributed through [GitHub Releases](https://github.com/DwennK/Tasks/releases) and the [DwennK Homebrew tap](https://github.com/DwennK/homebrew-tap). Both install the same universal macOS app for Apple Silicon and Intel.

```sh
brew install --cask dwennk/tap/tasks
```

If Homebrew asks for trust, review the [Tasks cask](https://github.com/DwennK/homebrew-tap/blob/main/Casks/tasks.rb), run `brew trust --cask dwennk/tap/tasks`, then retry the install. This grants trust only to this cask.

**Current builds are ad hoc signed, not Apple-notarized.** Without an Apple Developer subscription, macOS may require its normal Privacy & Security approval on first launch. Homebrew does not remove quarantine or bypass Gatekeeper. The updater uses a separate cryptographic signing key to authenticate each update.

## In the app

Open **Tasks → Check for Updates…** or **Settings → Updates**. Automatic checks run approximately 30 seconds after launch and every six hours while the app is open. Disable them in Settings if preferred. Failed background checks do not interrupt your work.

Updates are never downloaded or installed automatically. Review the notes, choose **Download update**, then **Install and restart**. Tasks validates the archive signature, asks every open window to finish saving, and waits for native operations before replacing the app. Cancelling or a failed save leaves the verified download ready to retry. Once installation begins, cancellation is disabled. The SQLite database stays in Application Support, outside the application bundle.

Development builds and custom `TASKS_DB` sessions cannot update the installed application. The browser preview shows explanatory UI only. Version 0.1.0 predates the updater and needs one manual installation of 0.2.0.

The update check requests the public GitHub release manifest; it does not send task data. Standard network metadata such as your IP address is visible to GitHub. Downloads follow GitHub's release asset redirects. Only signed `.app.tar.gz` assets announced under `DwennK/Tasks` are accepted.

Homebrew declares `auto_updates true`, allowing the app to update itself. To explicitly include it in a Homebrew upgrade:

```sh
brew upgrade --cask --greedy dwennk/tap/tasks
```

Removing the cask does not delete your task database; there is deliberately no `zap` rule for user data.

## Publishing a version

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and refresh `Cargo.lock`.
2. Update `docs/release-notes.md`, run the relevant checks and commit the release.
3. Push an annotated `vX.Y.Z` tag to GitHub. The release workflow tests the Rust workspace, checks TypeScript/Clippy and builds a universal app and DMG.
4. It verifies the bundle signature and both CPU architectures, then publishes the DMG, signed update archive, `latest.json`, SHA-256 checksums and generated `tasks.rb` together. A failed build never publishes the draft.
5. Update `Casks/tasks.rb` in `DwennK/homebrew-tap` from that release's generated file. The tap's Tasks workflow checks for new releases periodically and can also be dispatched manually.

Published releases must not be overwritten: publish a new version for fixes. Only one workflow runs per version tag. Pushing ordinary commits runs checks but does not release a new app.

## Signing keys

The updater public key is committed in `tauri.conf.json`. The corresponding private key is stored locally outside this repository at `~/.config/tasks/updater.key` and as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`. Keep a secure backup: losing it prevents existing installations from trusting future updates. Never commit or print it in build logs. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is optional when the key has a password.

Apple signing is independent. To enable Developer ID signing and notarization later, configure these repository secrets:

- `APPLE_CERTIFICATE`: base64-encoded Developer ID Application `.p12` export.
- `APPLE_CERTIFICATE_PASSWORD`: export password.
- `APPLE_SIGNING_IDENTITY`: full Developer ID Application identity.
- `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`: notarization credentials.

The release workflow requires notarization validation when a distribution identity is supplied. Without it, the workflow keeps ad hoc signing and appends the limitation to the release notes.

## Validation boundaries

Component tests model the native update boundary to cover errors, progress, signature-failure feedback and explicit installation consent. Rust checks cover allowed artifact locations and update state, while release metadata tests check architecture mapping, version agreement, checksums and cask safety. A successful build is not proof of an end-to-end installed update: exercise a transition between two real signed releases before claiming that result. Use an isolated Mac or VM for that test; production task data must not be used as a test fixture.
