# automerge-playground

Local-first bookmark manager with Automerge CRDT sync over Syncthing.

## Prerequisites

The project uses a **Nix flake** with [direnv](https://direnv.net/) to manage the development toolchain. Make sure you have Nix (with flakes enabled) and direnv installed, then:

```bash
direnv allow   # activates the dev shell automatically on cd
```

This provides cargo, rust-analyzer, openssl, pkg-config, cargo-deny, cargo-edit, and cargo-watch — no manual Rust installation required.

## Running

```bash
cargo run
```

### Environment Variables

| Variable | Description | Default |
|---|---|---|
| `BOOKMARK_SYNC_ROOT` | Shared sync folder (Syncthing) | `./sync_data` |
| `BOOKMARK_LOCAL_DIR` | Local data directory | `./local_data` |
| `BOOKMARK_PORT` | HTTP port (`0` = random free port) | `3000` |
| `BOOKMARK_DEV_MODE` | Enable dev mode (set to any value) | unset |
| `BOOKMARK_CLIENT_ID` | Override client ID | hostname (normal) or hostname+random (dev) |

### Client ID Resolution

In **normal mode** (default), the client ID is the machine hostname — stable across restarts, one instance per host.

In **dev mode** (`BOOKMARK_DEV_MODE=1`), you can run multiple instances on the same machine. Without an explicit `BOOKMARK_CLIENT_ID`, each launch gets a unique ephemeral ID (hostname + random suffix). Set `BOOKMARK_CLIENT_ID` for stable dev instances.

### Dev: Multiple Instances

```bash
# Terminal 1
BOOKMARK_DEV_MODE=1 BOOKMARK_CLIENT_ID=node-a BOOKMARK_PORT=0 \
  BOOKMARK_LOCAL_DIR=./local_a BOOKMARK_SYNC_ROOT=./sync_data cargo run

# Terminal 2
BOOKMARK_DEV_MODE=1 BOOKMARK_CLIENT_ID=node-b BOOKMARK_PORT=0 \
  BOOKMARK_LOCAL_DIR=./local_b BOOKMARK_SYNC_ROOT=./sync_data cargo run
```

### Parallel Agents / CI

Set `BOOKMARK_PORT=0` to let the OS assign a random free port — the actual port is printed to stderr on startup. This avoids collisions when multiple coding agents or CI jobs run in parallel. The E2E (Playwright) fixtures also discover a free port automatically before launching the server.

## Docker

Build and load the image (requires Nix):

    nix build .#docker && docker load < result

Run:

    docker run -p 3000:3000 \
      -v ./sync_data:/sync_data \
      -v ./local_data:/local_data \
      -e BOOKMARK_SYNC_ROOT=/sync_data \
      -e BOOKMARK_LOCAL_DIR=/local_data \
      automerge-playground:latest

## Validation

Run all checks before committing:

```bash
just validate
```

This runs: `fmt`, `clippy`, `test`, `deny`, `audit`, `doc` — the same checks CI enforces.

To include E2E (Playwright) tests as well:

```bash
just validate-all
```

Individual checks are also available (e.g. `just fmt`, `just test`). Run `just` to see all recipes.
