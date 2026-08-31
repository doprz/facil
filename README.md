# facil

[![crates.io](https://img.shields.io/crates/v/facil)](https://crates.io/crates/facil)

`facil` combines config-driven tmux session templating with a CLI session manager. Point it at a TOML file and it builds a tmux session from it - windows, panes, layouts, and setup commands, all declared once and repeatable every time.

Everything works headless and scriptable from the CLI; there's no TUI to get in the way.

## Why facil?

- **Single binary.** No Ruby/gem toolchain to install first (tmuxinator), no other language runtime - build or download one binary. The only other runtime dependency is `tmux` itself, and `facil doctor` confirms it's there and compatible.
- **TOML, not YAML.** Unambiguous parsing, no indentation footguns - a config's structure matches the data.
- **Bring your existing configs.** `facil import <tmuxinator.yml>` converts a tmuxinator project directly (see [docs/import.md](docs/import.md)) - switching costs an import and a quick review, not a rewrite.
- **The name**: *facil* comes from the Latin *facilis* / Spanish *fácil* - "easy" - the goal for both the config format and the CLI.

## Install

From source:

```sh
cargo install --path .
```

This will build and install `facil` in your `~/.cargo/bin`. Make sure that `~/.cargo/bin` is in your `$PATH` variable.

From crates.io:

```sh
cargo install facil
```

`facil` shells out to the `tmux` binary, so `tmux` itself needs to be on your `PATH`. Run `facil doctor` after installing to confirm your environment is set up correctly.

## Usage

A config is a TOML file describing a project's windows and panes:

```toml
name = "myproject"
root = "~/code/myproject"

[[windows]]
name = "editor"
layout = "main-vertical"

[[windows.panes]]
commands = ["nvim ."]

[[windows.panes]]
commands = ["cargo watch -x test"]

[[windows]]
name = "server"

[[windows.panes]]
commands = ["docker compose up"]
```

Save it as `~/.config/facil/myproject.toml` (or `./facil.toml` in a project directory, which is used automatically when no name is given), then:

```sh
facil start myproject   # builds the session (or attaches if it's already running)
facil stop myproject    # kills the session
```

### Commands

| Command | What it does |
|---|---|
| `facil start [name]` | Build (or attach to) a tmux session from a config |
| `facil stop [name]` | Kill a running session |
| `facil new [name]` | Scaffold a new config and open it in `$EDITOR` |
| `facil edit [name]` | Open a config in `$EDITOR` |
| `facil copy <existing> <new>` | Duplicate a config under a new name |
| `facil list` (alias `ls`) | Show configured projects and live tmux sessions - windows, panes, attached/uptime, and which running sessions have no matching config |
| `facil delete [name]` | Delete a config file |
| `facil validate [name]` | Check a config for errors without touching tmux |
| `facil debug [name]` | Print the tmux command sequence a `start` would run, without executing it |
| `facil doctor` | Check that tmux is installed, compatible, and the config dir is writable |
| `facil snapshot <session>` | Write a config that reproduces a live session's window/pane layout and working directories (not commands, `pre`/`post`, or `tmux_options`) |
| `facil import <path.yml> [name]` | Convert a tmuxinator YAML config to a facil config and open it in `$EDITOR` |

`name` is optional throughout: give one to operate on `~/.config/facil/<name>.toml`, or omit it to use `./facil.toml` in the current directory. `--config <path>` overrides both.

See [docs/config.md](docs/config.md) for the full config file spec - every field, validation rule, and the exact build order used when a session starts.

### Per-pane working directories

Any pane can override the project's `root`:

```toml
[[windows.panes]]
commands = ["npm run dev"]
root = "~/code/myproject/frontend"
```

### Variable substitution

Configs can reference `{{var}}` placeholders, resolved at launch time with `--set`:

```toml
[[windows.panes]]
commands = ["git checkout {{branch}}"]
```

```sh
facil start myproject --set branch=main
```

An unresolved `{{var}}` is a hard error before any tmux state changes - `facil validate` and `facil debug` catch these too.

## Shell completions

```sh
facil completions bash > /etc/bash_completion.d/facil
facil completions zsh  > ~/.zfunc/_facil
facil completions fish > ~/.config/fish/completions/facil.fish
```

(`elvish` and `powershell` are also supported.)

## Contributing

Issues and pull requests are welcome. Please run `cargo test` and `cargo clippy` before submitting.

## License

`facil` is licensed under the MIT License.

SPDX-License-Identifier: `MIT`. See [LICENSE](LICENSE) for the full text. By submitting a contribution, you agree it is licensed under the same terms.
