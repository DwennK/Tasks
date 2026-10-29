# Browser integration tests

Build the real CLI before running the browser suite:

```sh
cargo build -p tasks-cli --bin tasks
PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 pnpm test:ui
```

The suite launches installed Google Chrome with an isolated profile. It starts
its own Vite server on port 1421 and talks to the actual Rust task service through
the development bridge. Application workflows use no service or database mocks.
The dedicated RichNotes lifecycle regression mounts the real component in a
minimal browser document so focus and unmount can occur in the same event turn.

Fixtures are kept in `output/ui-test.sqlite3`, separate from the application data.
Names are unique and settings return to a known baseline before each test. Test
tasks remain available for failure investigation. Run one suite at a time, and
keep application sources stable during a run so hot reload does not invalidate
interactions.

Screenshots, traces on failure, and browser diagnostics are written under
`output/playwright/`. Desktop uses 1470×923; the responsive test also uses 390×844.
The conflict test deliberately causes a failed write and verifies retained drafts
and retry. Other browser errors fail the suite.

These tests verify behavior and persistence. Native window lifecycle, notification
delivery, signing, and visual comparison with TickTick require separate checks.

The print regression uses installed Poppler (`pdftotext`) to verify the actual
Chrome-generated PDF contains the selected task and no unrelated task or sidebar
content. It observes the real browser `beforeprint` event; the macOS print sheet
and printer delivery are verified separately. Quick Add tests cover date/list
context, saved defaults, parsed metadata, explicit overrides, and mobile IME input.
