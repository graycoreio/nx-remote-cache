# @graycoreio/nx-remote-cache

An Nx plugin that gives a workspace a remote cache on Azure Blob Storage with no other setup. Before Nx runs tasks, it makes sure an [nx-remote-cache](https://github.com/graycoreio/nx-remote-cache) server is running locally for the workspace, and points Nx's self-hosted cache client at it.

## Setup

```sh
npm install -D @graycoreio/nx-remote-cache
```

```jsonc
// nx.json
{
  "plugins": [
    {
      "plugin": "@graycoreio/nx-remote-cache",
      "options": {
        "storageAccount": "<storage account>",
        "storageContainer": "<container>",
        // For GitHub Actions:
        "tenantId": "<Entra tenant id>",
        "writerClientId": "<writer identity client id>",
        "readerClientId": "<reader identity client id>"
      }
    }
  ]
}
```

None of these are secrets: what anyone can do is decided by their role on the container, enforced by Azure.

| Option | Default | |
|---|---|---|
| `storageAccount`, `storageContainer` | | Where the cache lives. The plugin does nothing until both are set. |
| `storagePrefix` | `v1` | Virtual directory in the container for cache entries. |
| `tenantId`, `writerClientId`, `readerClientId` | | The CI identities (see the server's deployment docs). Only used in GitHub Actions. |
| `access` | `auto` | Which CI identity to ask for: `auto` (the writer only on a push to the default branch), `read` or `write`. |

## How it authenticates

- **On a developer machine,** the server uses your own `az login`. Your role on the container decides what you can do: with Storage Blob Data Reader, storage refuses your uploads and Nx skips them.
- **In GitHub Actions,** the plugin requests the job's OIDC token, and the server exchanges it with Entra for the writer or reader identity. The job needs `permissions: id-token: write`. Pull requests from forks get no OIDC token, so they build without the remote cache.

If the server can't start (no `az login`, no role on the container, no OIDC token), Nx runs without the remote cache instead of failing, and says why.

## Checking the cache

```sh
nx @graycoreio/nx-remote-cache
```

```
nx-remote-cache  ✔ connected (24 ms)
  storage   mystorageacct/nx-cache/v1
  identity  az login · you@example.com
  server    http://127.0.0.1:41237 (pid 1234)
  read      ✔ ok
  write     ✔ ok
```

The plugin adds this target, named after the package, to the workspace's root project. Running it starts the server like any other Nx command, then writes and reads a small entry through it, as Nx would (always the same one, so checks add nothing to the container). When the cache is off, it says why, with the end of the server's log, and fails. A read-only identity shows as a warning, or as expected for the CI reader. `npx nx-remote-cache status` checks the server the last Nx command started, without starting one.

With the Nx daemon running (the default outside CI), the plugin's own messages go to the daemon's log (`.nx/workspace-data/d/daemon.log`), not your terminal; this target is how to see them.

## Environment

| Variable | |
|---|---|
| `NX_REMOTE_CACHE=off` | Don't start a server; Nx runs without the remote cache. |
| `NX_REMOTE_CACHE_ACCESS` | Overrides the `access` option, e.g. `read` for a job that must never write. |
| `NX_REMOTE_CACHE_BINARY` | Runs this server binary instead of the bundled one, e.g. one built from source. |
| `NX_SELF_HOSTED_REMOTE_CACHE_SERVER` | If you set this yourself (your own server), the plugin leaves it alone. |
| `NX_VERBOSE_LOGGING=true` | Also logs why the cache is off when that is expected (e.g. not configured). |

With the Nx daemon running, the plugin runs inside the daemon and sees the environment the daemon started with. After changing these, run `nx daemon --stop`.

## The server process

The server keeps running after Nx exits, so later commands don't wait for it to start. A command reuses it while it answers and was started with the same binary and settings; otherwise the plugin starts a new one and stops the old one. Its log and state are in `.nx/gc-remote-cache/` in the workspace, which `nx reset` leaves alone.

```sh
npx nx-remote-cache status             # check the server the last Nx command started
npx nx-remote-cache stop               # stop this workspace's server
npx nx-remote-cache generate-token     # any other arguments go to the server binary
```

## Platforms

Linux x64 and arm64 (glibc 2.34 or newer) and macOS. Each platform's binary is a separate package, an optional dependency of this one, so npm installs only yours; don't install with `--omit=optional`.
