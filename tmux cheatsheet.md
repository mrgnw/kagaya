# tmux cheatsheet

When you `connect` to a process, you're inside a tmux session. The prefix key is `Ctrl+b` (press Ctrl+b first, release, then press the next key).

## Basics

| Keys | Action |
|------|--------|
| `Ctrl+b`, `d` | Detach (exit back to your shell) |
| `Ctrl+c` | Send interrupt to the running process |

## Scrolling / copy mode

| Keys | Action |
|------|--------|
| `Ctrl+b`, `[` | Enter scroll mode |
| `q` or `Esc` | Exit scroll mode |
| Up/Down or `k`/`j` | Scroll line by line |
| `Ctrl+u` / `Ctrl+d` | Scroll half page up/down |
| `g` / `G` | Jump to top/bottom of history |

## Selecting and copying text in scroll mode

| Keys | Action |
|------|--------|
| `Space` | Start selection |
| Arrow keys or `h`/`j`/`k`/`l` | Expand selection |
| `Enter` | Copy selection to tmux buffer |
| `Ctrl+b`, `]` | Paste tmux buffer |

## Typical workflow: copy an error

1. `ky connect web myapp` — attach to the process
2. `Ctrl+b`, `[` — enter scroll mode
3. Scroll up to find the error
4. `Space` — start selecting
5. Move to end of error text
6. `Enter` — copy
7. `Ctrl+b`, `d` — detach
8. Paste wherever you need it

> On macOS Terminal/iTerm2, you can also hold `Option` and click-drag to select text directly, or use `Cmd+C` to copy the visible terminal content — no scroll mode needed if the error is still on screen.
