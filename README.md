# acp-desk

A standalone desktop chat app for [ACP](https://agentclientprotocol.com/)-compatible AI coding agents.

Think Claude Desktop, but the backend is any agent that speaks ACP — Claude Code (via the official ACP adapter), Gemini CLI, Codex CLI, and others.

## Status

**Pre-alpha.** A Rust PoC that drives an ACP agent end-to-end lives in `poc/`.
See [DESIGN.md](DESIGN.md) for the target architecture, milestones, and open questions.

## Layout

```
poc/          minimal Rust CLI that spawns an ACP agent and prints its streamed reply
DESIGN.md     product goals, stack, milestones, open questions
```

## License

MIT
