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

When a PR touches the frontend (HTML, CSS, JS, templates), attach before and after screenshots for both desktop and mobile viewports. Use the Chrome DevTools MCP (preferred) or Firefox DevTools MCP for manual testing, debugging, and taking screenshots.

## Mobile Viewport Screenshots

Use `chrome-devtools-mcp` for viewport screenshots. Its `emulate` tool uses Chrome's device metrics override (not window resizing), so it can emulate any viewport including phone sizes below 500px.

**Setup** (once per machine):

```
claude mcp add chrome-devtools -- npx -y chrome-devtools-mcp@latest --headless
```

**Procedure for mobile screenshots:**

1. `emulate` — viewport: `"375x812x2,mobile,touch"`
2. `navigate_page` — load the target URL
3. `take_screenshot`

**Procedure for desktop screenshots:**

1. `emulate` — viewport: `"1280x800x1"`
2. `navigate_page` — load the target URL
3. `take_screenshot`

**How to distinguish mobile from desktop:**

| Indicator | Desktop (> 768px) | Mobile (≤ 480px) |
|---|---|---|
| Sidebar | Always visible | Hidden (drawer) |
| Detail panel | Always visible | Hidden (drawer) |
| Hamburger menu (≡) | Hidden | Visible |
| Back/Forward buttons | Visible | Hidden |
| Search bar | Full text input | Collapsed to icon |
| Statusbar | Visible | Hidden |
| Touch targets | 28px buttons | 44px buttons |

**Why not Firefox DevTools MCP?** Firefox's `set_viewport_size` resizes the OS window, which is clamped to ~500 CSS px on macOS. This triggers the ≤768px breakpoint (hamburger menu, drawer panels) but cannot reach the ≤480px breakpoint (hidden back/forward, collapsed search). Chrome's `emulate` uses CDP device metrics override which has no such limitation.

## Project Context

See [README.md](README.md) for build prerequisites, launch parameters, and dev workflow.
