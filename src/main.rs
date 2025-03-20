use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::io::{Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};

// CLI arguments definition
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum ApiProvider {
    OpenAi,
    Claude,
}

#[derive(Parser)]
#[command(
    name = "mr-comment",
    author = "",
    version,
    about = "Generate GitLab MR comments from git diffs using AI",
    long_about = r#"Generate professional GitLab MR comments from git diffs using AI

Examples:
  # Generate comment using Claude (default)
  mr-comment --api-key YOUR_API_KEY

  # Generate comment using OpenAI
  mr-comment --provider openai --api-key YOUR_OPENAI_KEY

  # Generate comment for a specific commit
  mr-comment --commit a1b2c3d

  # Generate comment for a range of commits
  mr-comment --commit "HEAD~3..HEAD"

  # Read diff from file
  mr-comment --file path/to/diff.txt

  # Write output to file
  mr-comment --output mr-comment.md

  # Use a different model
  mr-comment --provider claude --model claude-3-haiku-20240307"#
)]
struct Cli {
    /// Commit or range to generate comment for (e.g. "HEAD" or "HEAD~3..HEAD")
    #[arg(short, long)]
    commit: Option<String>,

    /// Read diff from file instead of git command [cannot be used with --commit]
    #[arg(short, long, conflicts_with = "commit")]
    file: Option<PathBuf>,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// API key (can also use OPENAI_API_KEY or ANTHROPIC_API_KEY env var)
    #[arg(short = 'k', long = "api-key")]
    api_key: Option<String>,

    /// API provider to use
    #[arg(
        short = 'p',
        long = "provider",
        value_enum,
        default_value = "claude",
        value_name = "PROVIDER"
    )]
    provider: ApiProvider,

    /// API endpoint (defaults based on provider)
    #[arg(short, long)]
    endpoint: Option<String>,

    /// Model to use (defaults based on provider)
    #[arg(short, long)]
    model: Option<String>,

    /// Debug mode - estimate token usage and exit
    #[arg(long)]
    debug: bool,
}

// Configuration structure
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Config {
    openai_api_key: Option<String>,
    claude_api_key: Option<String>,
    openai_endpoint: Option<String>,
    claude_endpoint: Option<String>,
    openai_model: Option<String>,
    claude_model: Option<String>,
    provider: Option<String>,
}

// API response structures
#[derive(Deserialize, Debug)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize, Debug)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize, Debug)]
struct OpenAIMessage {
    content: String,
}

#[derive(Deserialize, Debug)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize, Debug)]
struct ClaudeContent {
    text: String,
    #[serde(rename = "type")]
    content_type: String,
}

impl Config {
    fn load() -> Result<Self> {
        let config_path = get_config_path()?;
        if !config_path.exists() {
            return Ok(Config {
                openai_api_key: None,
                claude_api_key: None,
                openai_endpoint: None,
                claude_endpoint: None,
                openai_model: None,
                claude_model: None,
                provider: None,
            });
        }

        let config_str = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        let config: Config = serde_json::from_str(&config_str)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

        Ok(config)
    }

}

fn get_config_path() -> Result<PathBuf> {
    let mut path = dirs::home_dir().context("Could not find home directory")?;
    path.push(".mr-comment");
    Ok(path)
}

// Prompt template
struct PromptTemplate {
    purpose: &'static str,
    instructions: &'static str, 
}

impl PromptTemplate {
    fn new() -> Self {
        PromptTemplate {
            purpose: "Create standard gitlab MR comment",
             instructions: "Carefully review the git-log previosuly provided and then Generate a concise, professional MR comment based on that git log. Use a structured format that includes
 •\tMR Title:\n A short 1 sentance summary for use in a gitlab MR title [dont include the title header]
 •\tMR Summary:\n A brief overview of the changes. [dont include the summary header]
 •\t## Key Changes:\n A bulleted list of major updates or improvements.
 •\t## Why These Changes:\n A short explanation of the motivation behind the changes.
 •\t## Review Checklist:\n A list of items for reviewers to verify. Use a markdown checkbox for each item
 •\t## Notes:\n Additional context or guidance.
 Follow the style of simplifying technical details while maintaining clarity and professionalism. ALWAYS add a blank line after each heading.

 ONLY produce the MR comment and no additional questions or prompts. The git diff may be truncated due to length - focus analysis on the provided sections.",
        }
    }

    fn system_message(&self) -> String {
        format!("{}\n\n{}", self.purpose, self.instructions)
    }
}

fn get_diff_from_git(cli: &Cli) -> Result<String> {
    let mut cmd = Command::new("git");

    if let Some(commit_str) = &cli.commit {
        // Check if it's a range
        if commit_str.contains("..") {
            cmd.args(["diff", commit_str]);
        } else if commit_str == "HEAD" {
            cmd.args(["diff", "HEAD"]);
        } else {
            // Single commit - compare with its parent
            cmd.args(["diff", &format!("{}^", commit_str), commit_str]);
        }
    } else {
        // Default to showing staged+unstaged changes
        cmd.args(["diff"]);
    }

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to execute git command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Git command failed: {}", stderr);
    }

    let diff = String::from_utf8(output.stdout)
        .context("Failed to parse git output as UTF-8")?;

    // Process diff to summarize new/deleted files and filter binaries
    let mut filtered_lines = Vec::new();
    let mut new_files = Vec::new();
    let mut deleted_files = Vec::new();
    let mut current_file = None;
    let mut in_delete = false;
    let mut in_new = false;

    for line in diff.lines() {
        if line.starts_with("Binary files") {
            continue;
        }

        if line.starts_with("diff --git") {
            // Check if previous file was new/deleted
            if let Some(file) = current_file.take() {
                if in_new {
                    new_files.push(file);
                } else if in_delete {
                    deleted_files.push(file);
                }
            }

            // Reset state for new diff block
            in_delete = false;
            in_new = false;
            current_file = line.split(' ').nth(2).map(|s| s.trim_start_matches("a/").to_string());
            continue;
        }

        // Detect new/deleted file markers
        if line.starts_with("+++ /dev/null") {
            in_delete = true;
        } else if line.starts_with("--- /dev/null") {
            in_new = true;
        }

        // Only keep non-new/non-deleted file chunks
        if !in_new && !in_delete {
            filtered_lines.push(line);
        }
    }

    // Add any remaining file
    if let Some(file) = current_file.take() {
        if in_new {
            new_files.push(file);
        } else if in_delete {
            deleted_files.push(file);
        }
    }

    // Build summary of new/deleted files
    let mut summary = String::new();
    if !new_files.is_empty() {
        summary += "\nNew files:\n";
        for file in new_files {
            summary += &format!("• {}\n", file);
        }
    }
    if !deleted_files.is_empty() {
        summary += "\nDeleted files:\n";
        for file in deleted_files {
            summary += &format!("• {}\n", file);
        }
    }

    let mut filtered_diff = filtered_lines.join("\n");
    filtered_diff += &summary;

    if filtered_diff.trim().is_empty() {
        anyhow::bail!("No diff content found");
    }

    Ok(filtered_diff)
}

fn truncate_diff(diff: &str, max_lines: usize) -> (String, usize) {
    let lines: Vec<&str> = diff.lines().collect();
    let original_len = lines.len();
    if lines.len() <= max_lines {
        return (diff.to_string(), original_len);
    }
    
    // Keep beginning and end of diff since most relevant content is there
    let truncated = lines[..max_lines/2].join("\n")
        + "\n[...diff truncated...]\n"
        + &lines[lines.len()-max_lines/2..].join("\n");
    
    (truncated, original_len)
}

fn estimate_tokens(text: &str) -> usize {
    // Claude counts ~4 chars per token, OpenAI ~3.5 - we'll use conservative estimate
    (text.len() as f64 / 3.5).ceil() as usize
}

fn generate_mr_comment(
    diff: &str,
    api_key: &str,
    endpoint: &str,
    model: &str,
    provider: &ApiProvider,
    _check: bool,
) -> Result<String> {
    let client = Client::new();
    let prompt = PromptTemplate::new();

    // Truncate diff to 10k lines (keeps first/last 5000 lines)
    let (truncated_diff, original_len) = truncate_diff(diff, 10000);
    let diff_warning = if original_len > 10000 {
        format!(" (truncated from {} lines)", original_len)
    } else {
        String::new()
    };

    match provider {
        ApiProvider::OpenAi => {
            let request_body = json!({
                "model": model,
                "messages": [
                    {
                        "role": "system",
                        "content": prompt.system_message()
                    },
                    {
                        "role": "user",
                        "content": format!("Git diff{}:\n\n{}", diff_warning, truncated_diff)
                    }
                ],
                "temperature": 0.7
            });

            let response = client
                .post(endpoint)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&request_body)
                .send()
                .context("Failed to call OpenAI API")?;

            if !response.status().is_success() {
                let error_text = response.text().unwrap_or_else(|_| "Could not read error response".to_string());
                anyhow::bail!("OpenAI API request failed: {}", error_text);
            }

            let response_body: OpenAIResponse = response.json()
                .context("Failed to parse OpenAI API response")?;

            if response_body.choices.is_empty() {
                anyhow::bail!("OpenAI API response contained no choices");
            }

            Ok(response_body.choices[0].message.content.clone())
        },
        ApiProvider::Claude => {
            let request_body = json!({
                "model": model,
                "system": prompt.system_message(),
                "messages": [
                    {
                        "role": "user",
                        "content": format!("Git diff{}:\n\n{}", diff_warning, truncated_diff)
                    }
                ],
                "temperature": 0.7,
                "max_tokens": 4000
            });

            let response = client
                .post(endpoint)
                .header("Content-Type", "application/json")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request_body)
                .send()
                .context("Failed to call Claude API")?;

            if !response.status().is_success() {
                let error_text = response.text().unwrap_or_else(|_| "Could not read error response".to_string());
                anyhow::bail!("Claude API request failed: {}", error_text);
            }

            let response_body: ClaudeResponse = response.json()
                .context("Failed to parse Claude API response")?;

            if response_body.content.is_empty() {
                anyhow::bail!("Claude API response contained no content");
            }

            // Find the first text content
            for content in response_body.content {
                if content.content_type == "text" {
                    return Ok(content.text);
                }
            }

            anyhow::bail!("Claude API response contained no text content");
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = Config::load()?;

    // Get default values based on provider
    let (default_endpoint, default_model, env_var_key) = match cli.provider {
        ApiProvider::OpenAi => (
            "https://api.openai.com/v1/chat/completions",
            "gpt-4-turbo",
            "OPENAI_API_KEY"
        ),
        ApiProvider::Claude => (
            "https://api.anthropic.com/v1/messages",
            "claude-3-7-sonnet-20250219",
            "ANTHROPIC_API_KEY"
        ),
    };

    // Get API key from CLI, env var, or config
    let api_key = cli.api_key.clone()
        .or_else(|| env::var(env_var_key).ok())
        .or_else(|| {
            match cli.provider {
                ApiProvider::OpenAi => config.openai_api_key.clone(),
                ApiProvider::Claude => config.claude_api_key.clone(),
            }
        })
        .context(format!("API key is required. Provide it with --api-key or set {} environment variable", env_var_key))?;

    // Get endpoint from CLI or config
    let endpoint = cli.endpoint.clone().unwrap_or_else(|| {
        match cli.provider {
            ApiProvider::OpenAi => config.openai_endpoint.clone().unwrap_or_else(|| default_endpoint.to_string()),
            ApiProvider::Claude => config.claude_endpoint.clone().unwrap_or_else(|| default_endpoint.to_string()),
        }
    });

    // Get model from CLI or config
    let model = cli.model.clone().unwrap_or_else(|| {
        match cli.provider {
            ApiProvider::OpenAi => config.openai_model.clone().unwrap_or_else(|| default_model.to_string()),
            ApiProvider::Claude => config.claude_model.clone().unwrap_or_else(|| default_model.to_string()),
        }
    });


    // Get the diff
    let diff = if let Some(file_path) = cli.file {
        let mut file = fs::File::open(&file_path)
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
        content
    } else {
        get_diff_from_git(&cli)?
    };

    // Generate MR comment
    // If in debug mode
    if cli.debug {
        let system_message = PromptTemplate::new().system_message();
        let (truncated_diff, original_len) = truncate_diff(&diff, 4000);
        let diff_tokens = estimate_tokens(&truncated_diff);
        let system_tokens = estimate_tokens(&system_message);
        
        println!("Token estimation:");
        println!("- System prompt: {} tokens", system_tokens);
        println!("- Diff content: {} tokens ({} lines)", diff_tokens, original_len);
        println!("- Total estimate: {} tokens", system_tokens + diff_tokens);
        println!("Claude's limit: 200,000 tokens");
        return Ok(());
    }

    let mr_comment = generate_mr_comment(&diff, &api_key, &endpoint, &model, &cli.provider, cli.debug)?;

    // Output result
    if let Some(output_path) = cli.output {
        fs::write(&output_path, &mr_comment)
            .with_context(|| format!("Failed to write to file: {}", output_path.display()))?;
        println!("MR comment written to {}", output_path.display());
    } else {
        println!("{}", mr_comment);
    }

    Ok(())
}
