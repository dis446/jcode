# Tool inventory

Tools the model can call in a jcode session. Names are exact; use them with
`codemode` (`tools.<name>(args)`) or `callTool(name, args)`.

## Current setup

Config (`~/.jcode/config.toml`, `[tools]`):

```toml
enabled = []
disabled = ["batch"]
disable_base_tools = false
mcp_tools = "auto"
codemode = true
```

Result: 29 advertised tools, no MCP servers connected. The table below is that
surface. See [Surface gates](#surface-gates) for registered tools withheld from
this session.

## Advertised tools

| Tool | Purpose |
| ---- | ------- |
| `agentgrep` | Search code and file names; grep, find, outline, trace modes. |
| `apply_patch` | Multi-file patch from Codex `*** Begin Patch` or unified diff. |
| `ask_user_question` | Branching questionnaire; blocks until answered or timed out. |
| `bash` | Run a shell command. |
| `bg` | List, inspect, wait for, or cancel background tasks. |
| `browser` | Browser automation and task handoff across Chrome, Firefox, Safari, Edge. |
| `calendar` | Google Calendar: list, view, create, update, delete events. |
| `codemode` | Sandboxed JavaScript that chains and parallelizes the same tools. |
| `compile_remote` | Compile in an isolated Linux sandbox on subscription credits. |
| `conversation_search` | Search the current conversation history. |
| `debug_socket` | Send TUI debug socket commands (testers, frames, state). |
| `edit` | Exact-string replacement; all edits in one call apply atomically. |
| `gmail` | Gmail: search, read, draft, send, label, trash. |
| `integration_tools` | Search, inspect, and select product integrations. |
| `maintainer_feedback` | Send product feedback to the jcode maintainer. |
| `mcp` | Search, call, connect, and manage MCP servers and tools. |
| `memory` | Remember, recall, search, tag, and link stored notes. |
| `open` | Open or reveal a file, folder, or URL for the user. |
| `panel` | Spawn and update desktop Markdown/PDF panels. |
| `read` | Read text files, images, and PDFs. |
| `schedule` | Create, list, or cancel future scheduled tasks. |
| `selfdev` | Build, test, reload, and inspect the jcode CLI checkout. |
| `session_search` | Search past chat sessions across providers. |
| `skill_manage` | List, load, reload, and read skills. |
| `swarm` | Spawn and coordinate worker agents against todo lanes. |
| `todo` | Plan, update, and assess todos and goal completion. |
| `webfetch` | Fetch a URL as text, Markdown, or HTML. |
| `websearch` | Web search. |
| `write` | Write a file. |

## Surface gates

The registry holds more tools than a given session advertises. Gates:

- `batch` is registered but disabled by `[tools] disabled`.
- `side_panel` is a legacy panel tool with `model_visible() == false`; `panel` is
  the advertised equivalent.
- `desktop_selfdev` is removed outside a Desktop source checkout.
- `jcode_docs` is removed in canary/self-dev and Desktop modes; read the working
  tree instead.
- In Desktop self-dev mode, `selfdev` and `debug_socket` are also removed.
- `selfdev` and `debug_socket` are registered only by
  `register_selfdev_tools()`; regular sessions do not advertise them.
- `ask_user_question` is registered only for interactive clients that render the
  question dialog (TUI, Desktop), not headless or SDK sessions.
- `user_response` and `applet` are Desktop-only UI tools, registered only when
  Desktop UI is enabled.
- Ambient sessions add `end_ambient_cycle` and `schedule_ambient`.
- MCP tools are deferred behind the fixed `mcp` search/call surface unless
  `mcp_tools = "eager"`. Connected MCP tools are then callable as `mcp__<server>__<tool>`.
- Project tools from `$JCODE_HOME/tools.json` and `<repo>/.jcode/tools.json` join
  the surface at session attach. See `PROJECT_TOOLS.md`.
- SDK sessions may register custom tools, which shadow nothing and are not listed
  here.

`codemode` sees every registry tool a normal call could use, except `codemode`
itself and `batch`. It can call MCP tools by their bare name.

## Tool sources

1. Built-in Rust registry (`ToolRegistry`).
2. Project tools (`.jcode/tools.json`).
3. MCP servers (`mcp.json`, `.jcode/mcp.json`, `.claude/mcp.json`).
4. SDK custom tools per session.

A project entry replaces a global project entry with the same name, but a project
tool never overrides a built-in, MCP, or self-dev tool. If an MCP server announces
a name later, it replaces the project tool and a warning is logged.

## Related docs

- `PROJECT_TOOLS.md` — repository-defined tools.
- `ASK_USER_QUESTION.md` — interactive questionnaire tool and dialog.
- `SWARM_ARCHITECTURE.md` — worker spawning and coordination.
- `TODO_TASK_LIST.md` — todo state, pokes, and budgets.
