# Connect an AI agent to CleanCraft

CleanCraft includes an MCP server, so an AI app such as Claude Desktop or Claude Code can ask CleanCraft to look at your Mac and show you what it would clean. The server is the CleanCraft program itself, started with `serve`.

## What an agent can and cannot do

- With the connection alone, an agent can only look. It can scan, list your apps, read the Live Monitor figures, and prepare a cleanup list (a plan) with every file, its size and the reason.
- It cannot move anything. Carrying out a plan, turning a startup item off, or running a Tune-up Task is refused until you turn on **Let AI assistants clean after you confirm** in CleanCraft Settings, under AI assistants. This switch is off when you install CleanCraft. Leave it off unless you want agents to clean for you.
- Even with the switch on, an agent can only carry out a list you were shown. Every plan has a digest (a fingerprint of the exact items). The agent must send back the same digest it received with the preview; if anything in the list changed, CleanCraft refuses and the agent has to scan again and show you the new list.
- Nothing is ever deleted. Items go to the Trash or to CleanCraft's quarantine, and every action is in History with Undo, wherever it came from.

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
| `list_features` | CleanCraft's modules and their ids | No |
| `scan` | Scans one module and returns a plan: items, sizes, reasons, `plan_id` and `digest` | No |
| `get_plan` | Reads a saved plan, page by page | No |
| `sweep_preview` | Quick Sweep: every module at once, with a plan per cleanup module | No |
| `list_apps` | Installed apps with size, version, last opened and maker | No |
| `uninstall_preview` | The files of one app (or a reset), as a plan | No |
| `check_updates` | Newer versions of your apps and macOS (installs nothing) | No |
| `system_status` | CPU, memory, disk, Trash, battery and network now | No |
| `list_tasks` | Tune-up Tasks with their exact commands | No |
| `list_startup_items` | Login items, agents and daemons, and heavy apps | No |
| `full_disk_access` | Whether CleanCraft has Full Disk Access | No |
| `history` | Recent CleanCraft actions | No |
| `update_status` | CleanCraft's version, the latest release on GitHub and whether it is newer (installing happens in the app) | No |
| `execute_plan` | Carries out a plan you confirmed, with its `digest`; can leave items out of it, never add | Moves to Trash or quarantine; needs the switch |
| `run_task` | Runs one Tune-up Task you agreed to | Needs the switch |
| `toggle_startup_item` | Turns a startup item off or on (never one of macOS) | Needs the switch |
| `undo` | Puts back one action or a whole cleanup | Puts things back |

## The preview flow, step by step

1. The agent calls `scan` (or `sweep_preview`, `uninstall_preview`). Nothing changes.
2. It shows you the list: the files, sizes and reasons.
3. You say which items to clean, or that it is fine as shown.
4. Only then, and only if the switch is on, it calls `execute_plan` with the `plan_id` and the `digest` from step 1 (and, if you left some out, `only_items`).
5. CleanCraft checks the digest against the plan, moves the items to the Trash or quarantine, and records each move in History with Undo.

If you ever see an agent clean something you were not shown, turn the switch off and open History: everything can be put back from there.
