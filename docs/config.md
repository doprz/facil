# facil config file spec

This documents the TOML config format facil currently supports — every field, how
values are resolved, validation rules, and the exact order operations happen in
when a session is built. It reflects the implementation as it exists today, not
aspirational syntax.

## File discovery

A config is located one of three ways, in this order:

1. `--config <path>` — an explicit path, used verbatim regardless of any name.
2. `~/.config/facil/<name>.toml` — used when a `<name>` argument is given (e.g.
   `facil start myproject` looks for `~/.config/facil/myproject.toml`). This is a
   literal `~/.config/facil` path, not `$XDG_CONFIG_HOME`.
3. `./facil.toml` — used when no name and no `--config` are given, resolved
   relative to the current working directory.

The tmux session name is always the `name` field *inside* the loaded file, not
the filename or the `<name>` argument used to find it. In practice these should
match, but nothing enforces it — `facil start foo` can load a file whose `name`
is `"bar"`, and the resulting tmux session will be called `bar`.

## Top-level fields

```toml
name = "myproject"
root = "~/code/myproject"
pre = ["docker compose pull"]
post = ["notify-send done"]
tmux_options = "-x 250 -y 50"
socket_name = "myproject"

[[windows]]
# ...
```

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | yes | — | The tmux session name. Must not be empty. |
| `root` | string | no | none | Base working directory for the project. [Tilde-expanded](#root-resolution); overridable per pane. |
| `pre` | array of strings | no | `[]` | Shell commands run once on the **host**, before any window beyond the first is created. |
| `post` | array of strings | no | `[]` | Shell commands run once on the **host**, after every window/pane/layout is set up, right before attaching. |
| `tmux_options` | string | no | none | Raw extra arguments appended to the `tmux new-session` call, e.g. `"-x 250 -y 50"`. |
| `socket_name` | string | no | none | Runs the session on an isolated tmux server: every tmux invocation for this project gets `-L <socket_name>`. `facil list`/`ls` only discovers unmanaged sessions on the default socket or a socket some known config already names — it can't enumerate arbitrary sockets it's never heard of. |
| `windows` | array of tables | no (schema) / **yes** (validation) | `[]` | The session's windows, in order. An absent or empty `windows` parses fine but fails `facil validate`/`start`/`debug` with "at least one window is required." |

`name` missing entirely (not just empty) is a TOML parse error, since it's a
non-optional field in the schema — you'll see a parse error, not a validation
message, in that case. An empty string (`name = ""`) *is* caught by validation.

## `[[windows]]`

```toml
[[windows]]
name = "editor"
layout = "main-vertical"
pre_window = ["source .venv/bin/activate"]

[[windows.panes]]
commands = ["nvim ."]
```

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | no | `window<n>` | The tmux window name. If omitted, facil generates `window1`, `window2`, ... by position. Must be unique across all windows in the file either way (windows are targeted by name, e.g. `session:editor`, not by index) — an explicit name that collides with a generated one is still a validation error. |
| `layout` | string | no | none | Passed verbatim to `tmux select-layout` after this window's panes and commands are set up. Any tmux-accepted value works (`main-vertical`, `even-horizontal`, a raw layout string, ...) — facil only checks it's non-empty if present, it does not validate it's a real layout. |
| `pre_window` | array of strings | no | `[]` | Shell commands sent into **every pane of this window** (via `tmux send-keys`), before that pane's own `commands`. See [known limitation](#known-limitations) if `panes` is empty. |
| `panes` | array of tables | no | `[]` | This window's panes, in order. If empty, the window still exists with tmux's single default pane, but no commands are ever sent to it. |

There is no window-level `root` — only the project (`root`) and individual panes
(`panes[].root`) can set a working directory. A window's effective root, for the
purpose of creating the window itself, is whatever its **first** pane resolves
to (see [Root resolution](#root-resolution)).

## `[[windows.panes]]`

```toml
[[windows.panes]]
commands = ["npm run dev"]
root = "~/code/myproject/frontend"
```

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `commands` | array of strings | no | `[]` | Shell commands sent into this pane, in order, via `tmux send-keys ... Enter`, after the window's `pre_window` commands. |
| `root` | string | no | none | Overrides the working directory for just this pane. Takes precedence over the project's `root`. [Tilde-expanded](#root-resolution) and validated to exist. |

## Root resolution

For a given pane, the effective root is:

```
pane.root  (if set)
  else project.root  (if set)
    else none  (tmux uses its own default cwd, generally the shell that started tmux)
```

Only `~` and `~/...` prefixes are expanded (via `$HOME`); other paths — absolute
or relative — are passed through to tmux as-is with no `$PWD`-relative
resolution or canonicalization on facil's side.

Every pane/window creation call passes its root explicitly via tmux's `-c` flag
(rather than relying on tmux's cwd-inheritance from a "current" pane), so
placement is deterministic regardless of window/pane order.

A window's own root — used only when the window itself is created (`new-session`
for the first window, `new-window` for the rest) — comes from that window's
*first* pane. Every subsequent pane in the window resolves its root
independently the same way.

## Variable substitution

Configs can reference `{{var}}` placeholders anywhere in the file:

```toml
[[windows.panes]]
commands = ["git checkout {{branch}}"]
```

Resolved at launch time with repeatable `--set key=value` flags, available on
`start`, `debug`, and `validate`:

```sh
facil start myproject --set branch=main --set port=8080
```

Mechanics:

- Substitution is a **raw text** find/replace over the entire file, run
  *before* TOML parsing — `{{var}}` can appear anywhere, though in a valid TOML
  file it will almost always sit inside a string value.
- Syntax is exactly `{{`, any content, `}}`; the content between is trimmed of
  whitespace to get the variable name, so `{{branch}}` and `{{ branch }}` are
  equivalent. There's no escaping, default-value syntax, or nesting — the first
  `}}` found closes the token.
- Any `{{name}}` left unresolved after substitution (no matching `--set`) is a
  hard error, reported *before* the file is even parsed as TOML — so a missing
  `--set` gives a clear "unresolved variable" message rather than a confusing
  TOML syntax error. Only the first unresolved variable found is reported per
  run.
- `stop`, `list`, `edit`, `delete`, `new`, `copy`, and `doctor` don't accept
  `--set` and never error on unresolved variables — they substitute against an
  empty variable map and leave any `{{...}}` as literal text wherever it lands.
- There's no environment-variable interpolation and no built-in variables
  (no automatic `{{pwd}}`, `{{name}}`, etc.) — only what's explicitly passed via
  `--set`.

## Validation rules

`facil validate` (and the load path used by `start`/`debug`) checks, collecting
**every** violation in one pass rather than stopping at the first:

1. `name` must not be empty (after trimming whitespace).
2. `windows` must contain at least one entry.
3. If `root` is set, it must resolve (after tilde-expansion) to a directory
   that exists on disk at validation time.
4. For each window:
   - `name` must not be empty.
   - `name` must be unique among all windows in the file.
   - If `layout` is set, it must not be empty.
   - For each pane, if `root` is set, it must resolve to an existing directory
     (same rule as project `root`).

Each violation is reported as `field: message`, e.g.:

```
root: directory does not exist: /home/user/code/missing
windows[1].name: duplicate window name `editor`
```

**Not validated**: that `layout` is a real tmux layout (tmux will error at
run time if it isn't), `tmux_options` syntax, `socket_name` characters, or the
shell syntax of any command list (`pre`, `post`, `pre_window`, `commands`) —
these are opaque strings to facil until they're handed to `sh`/tmux.

## Build order

When `facil start` builds a session (i.e. `tmux has-session` came back false),
this is the exact sequence:

1. `tmux new-session -d -s <name> -n <first window> [-c <root>] [tmux_options]`
2. Each `pre` command, in order, on the **host** via `sh -c` (not inside tmux).
   A non-zero exit aborts here — the session created in step 1 is left running,
   not rolled back.
3. Each window after the first: `tmux new-window -t <session>: -n <name> [-c <root>]`.
4. Each pane after the first, in every window (file order): `tmux split-window
   -t <window> [-c <root>]`. Every extra pane is split from that window's
   *first* pane, not chained from the most recently added one — see
   [known limitations](#known-limitations).
5. For every pane, in every window (file order): `pre_window` commands, then
   that pane's own `commands`, each sent via `tmux send-keys -t <pane> "<cmd>"
   Enter`. Targeting uses the real pane ID tmux reports at creation time (via
   `-P -F '#{pane_id}'`), never a computed index — this stays correct even if
   `base-index`/`pane-base-index` are customized in the user's `tmux.conf`.
6. For every window with a `layout` set: `tmux select-layout -t <window> <layout>`.
7. Each `post` command, in order, on the host — same semantics as `pre`.
8. Attach: `tmux attach-session -t <session>` (or `tmux switch-client` if
   already inside a tmux client, i.e. `$TMUX` is set). Skipped with `--no-attach`.

If the session is already running, none of the above happens — `start` attaches
(or switches, or reports "already running" under `--no-attach`) immediately.

`facil debug` prints this same sequence without executing it, using symbolic
`window.pane_index` targets in place of real pane IDs (which only exist once
tmux actually creates them).

## Known limitations

- **`pre_window` needs at least one declared pane.** `pre_window` commands are
  sent once per *declared pane* in the window. A window with `pre_window` set
  but `panes = []` (or omitted) will silently never send those commands —
  there's no pane in the config to send them to, even though tmux itself
  creates one implicitly.
- **No window-level `root`.** Only `root` (project) and `panes[].root` exist.
  A window's effective root for its own creation is inherited from its first
  pane's resolved root.
- **3+ panes without a `layout` split unpredictably.** Every pane after the
  first is split from the window's first pane, not from the previously split
  one, and no `-h`/`-v` direction is passed — tmux's configured default split
  direction applies. Set `layout` on any window with more than two panes to get
  a deterministic arrangement.
- **`tmux_options` only splits on whitespace.** There's no shell-style quoting,
  so an argument containing a space can't be expressed (e.g. you can't pass a
  window name with a space via `tmux_options`).
- **Config files must be valid UTF-8.** Substitution and parsing both operate
  on the file as a UTF-8 string.

## Full example

```toml
name = "myproject"
root = "~/code/myproject"
pre = ["docker compose pull"]
post = ["notify-send 'myproject session ready'"]
tmux_options = "-x 220 -y 50"

[[windows]]
name = "editor"
layout = "main-vertical"

[[windows.panes]]
commands = ["nvim ."]

[[windows.panes]]
commands = ["cargo watch -x test"]

[[windows]]
name = "server"
pre_window = ["source .env"]

[[windows.panes]]
commands = ["docker compose up"]

[[windows]]
name = "frontend"

[[windows.panes]]
commands = ["git checkout {{branch}}", "npm run dev"]
root = "~/code/myproject/frontend"
```

Started with:

```sh
facil start myproject --set branch=main
```
