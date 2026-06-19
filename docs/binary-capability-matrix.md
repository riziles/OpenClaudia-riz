# Binary Capability Matrix

Each row corresponds to a command advertised in the README CLI command block.
Cells use one of three explicit states:

- `works`: implemented for this entrypoint and covered by focused tests or smoke tests.
- `unsupported: ...`: intentionally not implemented for this entrypoint; the note must name why.
- `not_applicable`: the entrypoint does not exercise that capability.

| README invocation | Entrypoint | Startup smoke | Provider policy | Tool policy/hooks | Grounding/final gate | Notes |
|---|---|---|---|---|---|---|
| `openclaudia` | default_tui | works: default full-screen TUI starts after config/auth resolution | works: direct provider requests use ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Default interactive path. |
| `openclaudia -m <model>` | default_tui | works: model override is accepted before TUI launch | works: overridden model is checked by ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Provider may be auto-detected from model. |
| `openclaudia -v` | default_tui | works: verbose flag enables logging before TUI launch | works: direct provider requests use ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Logging flag only changes diagnostics. |
| `openclaudia --resume` | default_tui | works: resume selects latest saved session before TUI launch | works: direct provider requests use ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Fails with documented error when no matching session exists. |
| `openclaudia --session-id <id>` | default_tui | works: explicit session id takes precedence over resume | works: direct provider requests use ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Prefix matching is intentional. |
| `openclaudia --coordinator --tui-mode` | legacy_repl | works: coordinator is accepted only with legacy REPL | works: legacy REPL provider requests use ProviderRequestPolicy | works: legacy REPL tool calls use shared ToolExecutor gates | works: legacy REPL finals use structured/cited final gate | Coordinator remains experimental. |
| `openclaudia --tui-mode` | legacy_repl | works: launches line-oriented legacy REPL | works: legacy REPL provider requests use ProviderRequestPolicy | works: legacy REPL tool calls use shared ToolExecutor gates | works: legacy REPL finals use structured/cited final gate | Additional legacy slash commands live here. |
| `openclaudia --mode <preset>` | default_tui | works: supported behavior preset is accepted before TUI launch | works: direct provider requests use ProviderRequestPolicy | works: local tool calls use shared ToolExecutor gates | works: agentic finals use structured/cited final gate | Clap rejects unknown presets. |
| `openclaudia --print "prompt"` | print | works: sends one prompt and exits | works: one-shot request uses ProviderRequestPolicy | not_applicable | unsupported: print mode has no tools, reality ledger, or agentic final gate | Non-interactive compatibility mode. |
| `openclaudia init` | init | works: creates config when absent | not_applicable | not_applicable | not_applicable | Filesystem-only setup command. |
| `openclaudia init --force` | init | works: overwrites existing config intentionally | not_applicable | not_applicable | not_applicable | Requires explicit force. |
| `openclaudia auth` | auth | works: starts OAuth/auth flow | not_applicable | not_applicable | not_applicable | Interactive authentication command. |
| `openclaudia auth --status` | auth | works: reports auth state without starting OAuth | not_applicable | not_applicable | not_applicable | Read-only auth check. |
| `openclaudia auth --logout` | auth | works: clears native OAuth cache | not_applicable | not_applicable | not_applicable | Does not delete shared Claude credentials. |
| `openclaudia start` | proxy | works: starts proxy server after config/auth resolution | works: proxy requests use ProviderRequestPolicy | not_applicable | not_applicable | Proxy forwards provider protocol. |
| `openclaudia start -p 9090` | proxy | works: port override is applied before bind | works: proxy requests use ProviderRequestPolicy | not_applicable | not_applicable | Bind errors are surfaced. |
| `openclaudia start -t openai` | proxy | works: target override is applied before auth preflight | works: proxy requests use ProviderRequestPolicy | not_applicable | not_applicable | Clap validates supported providers. |
| `openclaudia acp` | acp | works: starts ACP server on stdio | works: ACP provider requests use ProviderRequestPolicy | works: ACP tool calls use shared ToolExecutor gates | works: ACP finals use structured/cited final gate | ACP tool execution still prompts through ACP permission flow. |
| `openclaudia acp -m <model>` | acp | works: model override is accepted before ACP loop | works: overridden model is checked by ProviderRequestPolicy | works: ACP tool calls use shared ToolExecutor gates | works: ACP finals use structured/cited final gate | ACP model validation accepts configured/free-form models. |
| `openclaudia loop` | loop | works: starts iteration proxy mode | works: loop proxy requests use ProviderRequestPolicy | not_applicable | not_applicable | Stop hooks own iteration control. |
| `openclaudia loop -n 10` | loop | works: max-iteration override is applied | works: loop proxy requests use ProviderRequestPolicy | not_applicable | not_applicable | Zero means unlimited. |
| `openclaudia config` | config | works: prints current configuration | not_applicable | not_applicable | not_applicable | Redacts configured API keys. |
| `openclaudia doctor` | doctor | works: checks config/auth/connectivity | not_applicable | not_applicable | not_applicable | Avoids network probes when auth preflight fails. |
