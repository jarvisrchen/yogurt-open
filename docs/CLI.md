# Command line

`yogurt --help` and `yogurt <command> --help` are the source of truth; this is the same information in one place.

## `yogurt start`

Launches the local server and opens the browser.

| Flag | Default | What it does |
|------|---------|--------------|
| `--port <PORT>` | `7878` | TCP port to bind. Always binds `127.0.0.1` only, never a LAN interface. |
| `--no-open` | off | Do not auto-open `http://localhost:<port>` in your default browser once the server is listening. |
| `--dev` | off | Developer mode. Proxies every non-`/api` request to the Vite dev server on `:5173` instead of serving the web bundle embedded in the binary, loads `.env.local` from the current directory, and allows WebSocket upgrades from the `:5173` origin for hot reload. Never use this for normal use. |
| `--data-dir <PATH>` | `~/.yogurt` | Directory for the SQLite database, `keys.json`, the `notes/` folder, and `session-token`. Models still live under `~/.yogurt`. Also settable via `$YOGURT_DATA_DIR`; the flag wins if both are given. |

Without `--dev` the UI you see is whatever was compiled into the binary at build time.

## `yogurt doctor`

Prints a diagnostic dump (Rust and macOS versions, Screen Recording and Microphone permission state, configured providers, downloaded STT models) plus repair actions.

| Flag | What it does |
|------|--------------|
| `--json` | Emit the same diagnostics as JSON, safe to paste into a bug report. |
| `--check-port` | Report whether port 7878 is free or in use, and suggest a `--port` value if it is busy. |
| `--reset-screen-recording` | Reset the Screen Recording TCC grant for `ai.yogurt.app` so macOS prompts again on next start. |
| `--redownload-model <MODEL>` | Delete the local copy of a model's files (for example `parakeet-v3`) so the next start re-downloads it. |
| `--data-dir <PATH>` | Report the database under this directory instead of `~/.yogurt`. Also settable via `$YOGURT_DATA_DIR`. |

## `yogurt ctl`

Controls a *running* yogurt instance over HTTP - status, meetings, and meeting detection - without touching the UI.
Designed for an agent: subcommands rather than flag soup, `--json` output, descriptive errors with a `help:` line saying what to run instead.

```
yogurt ctl                                        # same as `status`
yogurt ctl status                                  # instances found, active/detected meeting, stt, provider, permission grants
yogurt ctl meeting list [--limit N]
yogurt ctl meeting new [--title T] [--notes-file <path>] [--start | --transcript-file <path> | --from-script <path>]
yogurt ctl meeting start <id|last>
yogurt ctl meeting stop [<id|url|last>]            # bare `stop` targets whatever is currently recording
yogurt ctl meeting show|summary|transcript <id|url|last>
yogurt ctl meeting enhance <id|url|last>
yogurt ctl meeting keep new|previous <id|url|last>  # after a re-enhance, keep its notes or put back the ones it replaced
yogurt ctl meeting mute on|off [<id|url|last>]     # pauses/resumes the mic; bare form targets whatever is recording
yogurt ctl meeting pause on|off [<id|url|last>]    # pauses/resumes the whole recording; bare form targets whatever is recording
yogurt ctl meeting search <query> [--limit N]      # full-text: title, notes, transcript
yogurt ctl meeting delete <id|url|last> [--dry-run] [--yes]
yogurt ctl detect [dismiss]                        # what meeting detection currently sees
yogurt ctl windows                                 # on-screen windows + each one's match verdict - no server needed
yogurt ctl settings get
yogurt ctl settings set k=v [k2=v2 ...] [--dry-run]
yogurt ctl provider list|activate <name>|test [<name>]
yogurt ctl models list|download <name> [--wait]|delete <name> [--dry-run]
yogurt ctl cloud status|push                       # Yogurt Cloud: connected or not, meetings not pushed, push now
yogurt ctl ws [<id|url|last>] [--types a,b] [--count N]   # one JSON frame per line, until Ctrl-C or --count
```

`<id|url|last>` accepts a bare meeting id, a full meeting URL (the port and id are read out of it), or the literal `last` for the most recently created meeting.
`list`/`show`/`summary`/`transcript` fall back to the local database when no server answers, noting `source: db` (stderr in text mode, a JSON field in `--json` mode) so you know the answer might be behind a server running on a different port.
Mutations are idempotent: starting an already-recording meeting or stopping one that isn't recording is a no-op, not an error.
`ws` follows the app-wide `/ws` socket by default (model download progress, chat chunks), or a meeting's own `/ws/meetings/<id>` socket (transcript, enhance progress) when given an id; `models download --wait` follows the same socket to print progress to stderr.

| Flag | What it does |
|------|--------------|
| `--json` | Print machine-readable JSON instead of compact text. |
| `--port <PORT>` | Talk to the instance on this port instead of discovering one. Also settable via `$YOGURT_PORT` (`just dev` prints the line to export). |

Without `--port`/`$YOGURT_PORT`, `ctl` scans `127.0.0.1:7878`-`7898` (the same window `just dev` uses when the default port is busy) for a live instance.
Two instances answering makes `status` list both; every other command exits 1 and asks you to pass `--port`.

`ctl` never sets or reveals an API key or the session token - use the Settings UI for keys, and see [AI-INTEGRATION.md](AI-INTEGRATION.md) if you need the token for a raw HTTP call `ctl` doesn't cover yet.
