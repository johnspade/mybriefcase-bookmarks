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

Use `chrome-devtools-mcp` for viewport screenshots. Its `emulate` tool uses Chrome's device metrics override (not window resizing), so it can emulate any viewport including phone sizes.

Viewport dimensions:

- **Mobile:** `emulate` with viewport `"375x812x2,mobile,touch"`, then `navigate_page`, then `take_screenshot`
- **Desktop:** `emulate` with viewport `"1280x800x1"`, then `navigate_page`, then `take_screenshot`

Do not use Firefox DevTools MCP for viewport screenshots — its `set_viewport_size` resizes the OS window, which macOS clamps to ~500 CSS px (cannot reach the ≤480px phone breakpoint).

## Project Context

See [README.md](README.md) for build prerequisites, launch parameters, and dev workflow.
