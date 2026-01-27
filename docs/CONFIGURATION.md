# Configuration Reference

> Environment variables, CLI commands, and setup options

---

## First-Run Network Requirement

Vestige downloads the **Nomic Embed Text v1.5** model (~130MB) from Hugging Face on first use.

**All subsequent runs are fully offline.**

Model cache location:
- Creates `.fastembed_cache/` in the current working directory on first run
- Contains symlinks to model files in `~/.cache/huggingface/`

**Recommended**: Run your first Vestige command from your home directory:
```bash
cd ~
vestige health   # Creates ~/.fastembed_cache/ once
```

Or set the environment variable:
```bash
export FASTEMBED_CACHE_PATH="$HOME/.fastembed_cache"
```

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `VESTIGE_DATA_DIR` | Platform default | Custom database location |
| `VESTIGE_LOG_LEVEL` | `info` | Logging verbosity |
| `RUST_LOG` | - | Detailed tracing output |
| `FASTEMBED_CACHE_PATH` | `./.fastembed_cache` | Embedding model cache location |

---

## Command-Line Options

```bash
vestige-mcp --data-dir /custom/path   # Custom storage location
vestige-mcp --help                     # Show all options
```

---

## CLI Commands (v1.1+)

Stats and maintenance were moved from MCP to CLI to minimize context window usage:

```bash
vestige stats              # Memory statistics
vestige stats --tagging    # Retention distribution
vestige stats --states     # Cognitive state distribution
vestige health             # System health check
vestige consolidate        # Run memory maintenance
vestige restore <file>     # Restore from backup
```

---

## Claude Configuration

### Claude Code (One-liner)

```bash
claude mcp add vestige vestige-mcp -s user
```

### Claude Code (Manual)

Add to `~/.claude/settings.json`:
```json
{
  "mcpServers": {
    "vestige": {
      "command": "vestige-mcp"
    }
  }
}
```

### Claude Desktop (macOS)

Add to `~/Library/Application Support/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "vestige": {
      "command": "vestige-mcp"
    }
  }
}
```

### Claude Desktop (Windows)

Add to `%APPDATA%\Claude\claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "vestige": {
      "command": "vestige-mcp"
    }
  }
}
```

---

## Custom Data Directory

For per-project or custom storage:

```json
{
  "mcpServers": {
    "vestige": {
      "command": "vestige-mcp",
      "args": ["--data-dir", "/path/to/custom/dir"]
    }
  }
}
```

See [Storage Modes](STORAGE.md) for more options.

---

## Updating Vestige

**Latest version:**
```bash
cd vestige
git pull
cargo build --release
sudo cp target/release/vestige-mcp /usr/local/bin/
```

**Pin to specific version:**
```bash
git checkout v1.1.1
cargo build --release
```

**Check your version:**
```bash
vestige-mcp --version
```

---

## Development

```bash
# Run tests
cargo test --all-features

# Run with logging
RUST_LOG=debug cargo run --release

# Build optimized binary
cargo build --release --all-features
```
