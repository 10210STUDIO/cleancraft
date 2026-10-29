# Connect an AI agent to CleanCraft

CleanCraft includes an MCP server, so an AI app such as Claude Desktop or Claude Code can ask CleanCraft to look at your Mac and show you what it would clean. The server is the CleanCraft program itself, started with `serve`.

## What an agent can and cannot do

- With the connection alone, an agent can only look. It can scan, list your apps, read the Live Monitor figures, and prepare a cleanup list (a plan) with every file, its size and the reason. It can also choose which items of a plan to clean, but choosing changes nothing on your Mac.
- Everything that changes your Mac is refused until you turn on **Let AI assistants clean after you confirm** in CleanCraft Settings, under AI assistants. This switch is off when you install CleanCraft, and no agent can turn it on: only you, in the app. The tool `agent_permissions` tells an agent whether it is on.
- With the switch on, an agent can do what the app does: clean any module, uninstall apps (quitting them first), remove or turn off startup items, run Tune-up Tasks, empty the Trash, reset app permissions, change settings, schedules and the ignore list, turn the menu bar agent on or off, and install a CleanCraft update. `list_actions` lists every action with the preview it needs.
- An agent can only carry out a list you were shown. Every plan has a digest (a fingerprint of the exact items selected). When the agent changes the selection with `select_items`, it gets a new digest and must show you the new list; `execute_plan` refuses any other digest.
- Items in folders of the system (for example `/Library/LaunchDaemons`) need the administrator password. macOS shows its own password or Touch ID dialog on your Mac, once for the whole run; you approve it there. CleanCraft never asks the agent for your password. If you cancel, those items are skipped and the rest goes on.
- Nothing is ever deleted by a cleanup. Items go to the Trash or to CleanCraft's quarantine, and every action is in History with Undo, wherever it came from. The only permanent actions are the ones you would also confirm in the app: emptying the Trash, erasing deleted Mail messages and resetting app permissions.

## The quick way: from CleanCraft

The first time CleanCraft opens, it asks whether to connect your AI apps. You can also open this step later from Settings, under AI assistants.

1. CleanCraft looks for Claude Desktop and Claude Code and says which it found.
2. Choose **Connect** next to each app you want.
   - Claude Desktop: CleanCraft adds one entry named `cleancraft` to Claude Desktop's settings file and changes nothing else. A copy of the previous file is kept in CleanCraft's folder (`~/Library/Application Support/CleanCraft/mcp-backups/`). If the file is not valid JSON, CleanCraft does not touch it and offers to open it for you.
   - Claude Code: CleanCraft runs `claude mcp add --scope user cleancraft -- <CleanCraft> serve`. Without the `claude` command it adds the same single entry to `~/.claude.json`.
3. Quit and reopen Claude Desktop. New Claude Code sessions see CleanCraft at once.
4. To take it back, use Undo in the notice, or Undo on the "Connected CleanCraft to ..." line in History. Only the `cleancraft` entry is removed.

The choice to let agents clean is a separate switch on the same screen, off by default.

## By hand

CleanCraft must be installed in Applications (`scripts/install-app.sh` puts it there and links `~/.local/bin/cleancraft`). The server command is:

```
/Applications/CleanCraft.app/Contents/MacOS/CleanCraft serve
```

### Claude Desktop

Open `~/Library/Application Support/Claude/claude_desktop_config.json` (Claude Desktop: Settings, Developer, Edit Config) and add `cleancraft` inside `mcpServers`, keeping the servers already there:

```json
{
  "mcpServers": {
    "cleancraft": {
      "command": "/Applications/CleanCraft.app/Contents/MacOS/CleanCraft",
      "args": ["serve"]
    }
  }
}
```

Save, then quit and reopen Claude Desktop.

### Claude Code

```
claude mcp add --scope user cleancraft -- /Applications/CleanCraft.app/Contents/MacOS/CleanCraft serve
claude mcp list
```

`claude mcp remove --scope user cleancraft` takes it away.

## The tools

| Tool | What it does | Changes anything? |
|---|---|---|
| `list_actions` | Every action an agent can take, whether it changes the Mac, the preview to show first and how to undo it | No |
| `agent_permissions` | Whether the AI assistants switch is on, and how you turn it on | No |
| `list_features` | CleanCraft's modules and their ids | No |
| `scan` | Scans one module and returns a plan: items, sizes, reasons, `plan_id` and `digest` | No |
| `get_plan` | Reads a saved plan, page by page | No |
| `select_items` | Sets the plan's selection to exactly the items named (also ones not ticked by default), saves it and returns the preview with the new `digest` | Only the saved plan |
| `sweep_preview` | Quick Sweep: every module at once (or the ones named), with a plan per cleanup module | No |
| `list_apps` | Installed apps with size, version, last opened and maker | No |
| `uninstall_preview` | The files of one app (or a reset), as a plan | No |
| `uninstall_apps_preview` | The files of several apps at once, as one plan | No |
| `app_running` | What of an app runs now and whether quitting it needs the password | No |
| `check_updates` | Newer versions of your apps and macOS (installs nothing) | No |
| `system_status` | CPU, memory, disk, Trash, battery and network now | No |
| `list_tasks` | Tune-up Tasks with their exact commands | No |
| `list_startup_items` | Login items, agents and daemons, and heavy apps | No |
| `trash_status` | The Trash bins and Mail's deleted messages, with sizes and the token for `empty_trash` | No |
| `quarantine_status` | What CleanCraft's quarantine holds | No |
| `get_settings` | CleanCraft's settings, ignore list, schedules and the menu bar agent | No |
| `full_disk_access` | Whether CleanCraft has Full Disk Access | No |
| `open_full_disk_access_settings` | Opens System Settings at Full Disk Access for you | Opens a window only |
| `history` | Recent CleanCraft actions | No |
| `update_status` | CleanCraft's version, the latest release on GitHub and whether it is newer | No |
| `execute_plan` | Carries out a plan you confirmed, with its `digest`; `only_items` can leave items out, never add | Moves to Trash or quarantine; needs the switch |
| `sweep_run` | Runs several confirmed Quick Sweep plans, each with its digest | Moves to Trash or quarantine; needs the switch |
| `quit_app` | Quits an app and its helpers (for example before uninstalling it) | Needs the switch |
| `quit_process` | Quits a heavy app from the Startup Items list | Needs the switch |
| `run_task` | Runs one Tune-up Task you agreed to | Needs the switch |
| `toggle_startup_item` | Turns a startup item off or on (never one of macOS) | Needs the switch |
| `empty_trash` | Asks Finder to empty the Trash (permanent) | Needs the switch |
| `erase_mail_deleted` | Asks Mail to erase one account's deleted messages (permanent) | Needs the switch |
| `reset_app_permissions` | Takes back access you gave to apps (camera, microphone, screen...) | Needs the switch |
| `quarantine_to_trash` | Moves items of the quarantine to the Trash | Needs the switch |
| `update_settings` | Changes settings you asked for (never the AI assistants switch) | Needs the switch |
| `ignore_list` | Adds paths to or takes paths off the ignore list | Needs the switch |
| `menu_bar_agent` | Turns the menu bar agent on or off | Needs the switch |
| `install_update` | Installs the newer CleanCraft (refused while the app is open) | Needs the switch |
| `undo` | Puts back one action or a whole cleanup | Puts things back |

## The preview flow, step by step

1. The agent calls `scan` (or `sweep_preview`, `uninstall_preview`, `uninstall_apps_preview`). Nothing changes.
2. It shows you the list: the files, sizes and reasons.
3. You say which items to clean. The agent calls `select_items` with exactly those (it can pick items that were not ticked) and shows you the preview it returns, including which items need the administrator password.
4. Only after your yes, and only if the switch is on, it calls `execute_plan` with the `plan_id` and the `digest` from that preview.
5. CleanCraft checks the digest against the plan, checks every item again against its safety rules, moves the items to the Trash or quarantine, and records each move in History with Undo. If some items need the administrator password, macOS asks you on the Mac.
6. The agent reports what moved, what was skipped and why, and the id to undo it.

## How an agent should confirm with the user

- Before any tool that changes the Mac, show the user the preview in plain words: every path with its size and reason, the total, where the items go, and whether macOS will ask for the administrator password.
- Ask a direct question and wait for a clear yes. "Clean these 4 items (12 KB) and move them to the Trash?" is good. Silence, or a yes to something else, is not a yes.
- If the user changes the list, call `select_items` again and show the new preview: its digest is the only one `execute_plan` accepts.
- For permanent actions (empty the Trash, erase deleted Mail messages, reset app permissions), say that they cannot be undone.
- When the password dialog appears, tell the user to approve it on the Mac. Never ask the user to type a password in the chat.
- If a tool answers "Refused" because the switch is off, explain how to turn it on (`agent_permissions`) or offer to let the user do it in the app. Do not look for another way around.

If you ever see an agent clean something you were not shown, turn the switch off and open History: everything can be put back from there.
