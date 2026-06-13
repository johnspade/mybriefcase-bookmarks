# Agent Guidelines

## Commit Messages

PR titles must follow [Conventional Commits](https://www.conventionalcommits.org/) — CI enforces this. The PR title becomes the squash-merge commit message on main, and drives automatic semantic versioning.

Format: `<type>: <description>` or `<type>(scope): <description>`

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.

Use `!` after the type for breaking changes: `feat!: remove legacy endpoint`.

## Before Committing

Run `just validate` before committing. All checks must pass — CI will reject the PR otherwise.

Never force-push to `main`. If a commit needs fixing, create a new commit instead of amending.

## Warnings

Fix compiler and clippy warnings properly instead of suppressing them with `#[allow(...)]` attributes. If a warning indicates dead code, remove it. If it flags a function as too long, refactor it. If it reports an unused async, restructure the handler. Silencing warnings hides real problems.

For genuine false positives (e.g. shared test utility modules triggering `dead_code` per-binary), use `#[expect(..., reason = "...")]` instead of `#[allow]` so the suppression self-documents and warns if it becomes unnecessary.

## Python

Python is not in the devShell. Use `uv` for one-off scripts with dependencies:

```
uv run --with <packages> script.py
```

## Running the Server

Always use `MBB_PORT=0` when launching the server (lets the OS pick a free port). Never hardcode ports like 3000, 3001, etc. — other agents may be running in parallel. Read the actual port from stderr output.

Never kill processes by port (e.g. `lsof -ti :PORT | xargs kill`). The user's Firefox, Docker, and other tools may be listening on the same ports. To stop a test server you started, kill its specific PID instead.

## Pull Requests

PR descriptions must follow the template in `.github/pull_request_template.md`.

When a PR touches the frontend (HTML, CSS, JS, templates), attach before and after screenshots for both desktop and mobile viewports. Use the Firefox devtools MCP (if available) for manual testing, debugging, and taking screenshots.

## Mobile Viewport Screenshots

Use `set_viewport_size` + `navigate_page` (in that order) to trigger mobile layout. Setting the viewport after the page is already loaded won't re-trigger CSS media queries for elements that are already rendered.

**Procedure for mobile screenshots:**

1. `set_viewport_size` — width: 375, height: 812
2. `navigate_page` (or reload) — the page must load *after* the viewport is set
3. `screenshot_page`

**Procedure for desktop screenshots:**

1. `set_viewport_size` — width: 1280, height: 800
2. `navigate_page` (or reload)
3. `screenshot_page`

**How to distinguish mobile from desktop:**

| Indicator | Desktop (> 768px) | Mobile (≤ 768px) |
|---|---|---|
| Sidebar | Always visible | Hidden (drawer, opens via ≡) |
| Detail panel | Always visible | Hidden (drawer) |
| Hamburger menu (≡) | Hidden | Visible |
| Touch targets | 28px buttons | 44px buttons |

**Limitations:**

- Firefox enforces a minimum window width of ~500 CSS px on macOS. Requesting smaller values (e.g. 375) will clamp to ~500px. This still triggers the `≤ 768px` breakpoint (the primary mobile layout), so it works for screenshots.
- The `≤ 480px` breakpoint (hides back/forward buttons, collapses search to icon) cannot be triggered via the MCP on macOS due to this minimum. If you need to verify those styles, use the e2e test suite (Playwright) which runs headless without this constraint.
- If `set_viewport_size` appears to have no effect, `restart_firefox` and then set the viewport *before* the first navigation. Firefox may cache the initial window size from a prior session.

## Project Context

See [README.md](README.md) for build prerequisites, launch parameters, and dev workflow.
