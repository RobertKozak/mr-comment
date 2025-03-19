# MR Comment Generator (Rust)

A command-line tool written in Rust that generates professional GitLab Merge Request (MR) comments based on git diffs using AI (OpenAI or Claude).

## Features

- Reads git diffs from current repo or from a file
- Supports both OpenAI and Claude (Anthropic) APIs
- Customizable API endpoint and model
- Outputs to console or to a file
- Saves configuration for ease of use
- Native binary with no runtime dependencies (thanks to Rust)

## Installation

### Prerequisites

- Rust and Cargo (install via [rustup](https://rustup.rs/))
- Git
- OpenAI API key or Claude API key

### Building from source

```bash
# Clone the repository
git clone https://github.com/yourusername/mr-comment.git
cd mr-comment

# Build in release mode
cargo build --release

# The binary will be available at target/release/mr-comment
```

### Installing with Cargo

```bash
# Install directly from GitHub (if repository is public)
cargo install --git https://github.com/yourusername/mr-comment.git

# Or install locally after cloning
cargo install --path .
```

## Usage

```bash
# Generate comment using OpenAI (default)
mr-comment --api-key YOUR_OPENAI_API_KEY

# Generate comment using Claude
mr-comment --provider claude --api-key YOUR_CLAUDE_API_KEY

# Generate comment for a specific commit
mr-comment --commit a1b2c3d

# Generate comment for a range of commits
mr-comment --range "HEAD~3..HEAD"

# Read diff from file
mr-comment --file path/to/diff.txt

# Save API key to config (won't need to provide it in future commands)
mr-comment --api-key YOUR_API_KEY --provider claude --save-config

# Write output to file
mr-comment --output mr-comment.md

# Use a different model
mr-comment --provider claude --model claude-3-haiku-20240307
```

### Options

- `-c, --commit <COMMIT>`: Specific commit to generate comment for (default: HEAD)
- `-f, --file <FILE>`: Read diff from file instead of git command
- `-o, --output <FILE>`: Write output to file instead of stdout
- `-k, --api-key <API_KEY>`: API key (can also use OPENAI_API_KEY or ANTHROPIC_API_KEY env var)
- `-p, --provider <PROVIDER>`: API provider to use (openai or claude)
- `-e, --endpoint <ENDPOINT>`: API endpoint (defaults based on provider)
- `-m, --model <MODEL>`: Model to use (defaults based on provider)
- `-s, --save-config`: Save API key and endpoint to config file
- `-r, --range <RANGE>`: Git diff range (e.g., "HEAD~3..HEAD")
- `-h, --help`: Print help
- `-V, --version`: Print version

## Configuration

The tool will look for configuration in the following order:

1. Command line arguments
2. Environment variables (`OPENAI_API_KEY` for OpenAI or `ANTHROPIC_API_KEY` for Claude)
3. Saved configuration file (`~/.mr-comment`)

You can save your API key and preferred endpoint to a configuration file using the `--save-config` option.

### Default Values

#### OpenAI

- Endpoint: `https://api.openai.com/v1/chat/completions`
- Model: `gpt-4-turbo`

#### Claude

- Endpoint: `https://api.anthropic.com/v1/messages`
- Model: `claude-3-opus-20240229`

## Example Output

```markdown
## MR Title: Implement user authentication system

### MR Summary

This MR adds a complete user authentication system including login, registration, password reset, and account management.

### Key Changes

- Added user model with secure password hashing
- Implemented JWT-based authentication
- Created login and registration API endpoints
- Added password reset functionality
- Included comprehensive test coverage

### Why These Changes

These changes provide a secure authentication foundation for the application, allowing users to create accounts and access protected features.

### Review Checklist

- [ ] Verify all authentication routes are properly protected
- [ ] Check password hashing implementation
- [ ] Review JWT token expiration and refresh logic
- [ ] Confirm test coverage for edge cases
- [ ] Validate error handling for invalid credentials

### Notes

The implementation follows OWASP security guidelines and includes rate limiting to prevent brute force attacks.
```

## Development

### Project Structure

- `src/main.rs`: Contains all the code for the CLI tool
- `Cargo.toml`: Rust package configuration and dependencies

### Dependencies

- `clap`: Command line argument parsing
- `reqwest`: HTTP client for API calls
- `serde` and `serde_json`: JSON serialization/deserialization
- `anyhow`: Error handling
- `dirs`: Finding user's home directory
- `tokio`: Async runtime (although we're using reqwest in blocking mode)

## License

MIT
