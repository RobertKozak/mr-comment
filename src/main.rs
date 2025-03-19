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
enum ApiProvider {
    OpenAi,
    Claude,
}

#[derive(Parser)]
#[command(
    name = "mr-comment",
    author = "",
    version,
    about = "Generate GitLab MR comments from git diffs using AI"
)]
struct Cli {
    /// Specific commit to generate comment for (default: HEAD)
    #[arg(short, long)]
    commit: Option<String>,

    /// Read diff from file instead of git command
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// API key (can also use OPENAI_API_KEY or ANTHROPIC_API_KEY env var)
    #[arg(short = 'k', long = "api-key")]
    api_key: Option<String>,

    /// API provider to use
    #[arg(short = 'p', long = "provider", value_enum, default_value = "openai")]
    provider: ApiProvider,

    /// API endpoint (defaults based on provider)
    #[arg(short, long)]
    endpoint: Option<String>,

    /// Model to use (defaults based on provider)
    #[arg(short, long)]
    model: Option<String>,

    /// Save API key and endpoint to config file
    #[arg(short, long)]
    save_config: bool,

    /// Git diff range (e.g., "HEAD~3..HEAD")
    #[arg(short, long)]
    range: Option<String>,
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

    fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;
        let config_str = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
        
        fs::write(&config_path, config_str)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;
        
        println!("Config saved successfully.");
        Ok(())
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
            instructions: "Carefully review the git-log previosuly provided and then Generate a concise, professional MR comment based on that git log. Use a structured format that includes:
•\tMR Title: A short 1 sentance summary for use in a gitlab MR title
•\tMR Summary: A brief overview of the changes.
•\tKey Changes: A bulleted list of major updates or improvements.
•\tWhy These Changes: A short explanation of the motivation behind the changes.
•\tReview Checklist: A list of items for reviewers to verify. Use a markdown checkbox for each item
•\tNotes: Additional context or guidance.
Follow the style of simplifying technical details while maintaining clarity and professionalism. 

ONLY produce the MR comment and no additional questions or prompts",
        }
    }

    fn system_message(&self) -> String {
        format!("{}\n\n{}", self.purpose, self.instructions)
    }
}

fn get_diff_from_git(commit: Option<&str>, range: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("git");
    
    if let Some(range_str) = range {
        cmd.args(["diff", range_str]);
    } else if let Some(commit_str) = commit {
        cmd.args(["show", commit_str]);
    } else {
        cmd.args(["show", "HEAD"]);
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
    
    if diff.trim().is_empty() {
        anyhow::bail!("No diff content found");
    }
    
    Ok(diff)
}

fn generate_mr_comment(
    diff: &str,
    api_key: &str,
    endpoint: &str,
    model: &str,
    provider: &ApiProvider,
) -> Result<String> {
    let client = Client::new();
    let prompt = PromptTemplate::new();
    
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
                        "content": format!("Git diff:\n\n{}", diff)
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
                        "content": format!("Git diff:\n\n{}", diff)
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
    let mut config = Config::load()?;
    
    // Get default values based on provider
    let (default_endpoint, default_model, env_var_key) = match cli.provider {
        ApiProvider::OpenAi => (
            "https://api.openai.com/v1/chat/completions",
            "gpt-4-turbo",
            "OPENAI_API_KEY"
        ),
        ApiProvider::Claude => (
            "https://api.anthropic.com/v1/messages",
            "claude-3-opus-20240229",
            "ANTHROPIC_API_KEY"
        ),
    };

    // Get API key from CLI, env var, or config
    let api_key = cli.api_key
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
    
    // Save config if requested
    if cli.save_config {
        match cli.provider {
            ApiProvider::OpenAi => {
                config.openai_api_key = Some(api_key.clone());
                config.openai_endpoint = Some(endpoint.clone());
                config.openai_model = Some(model.clone());
            },
            ApiProvider::Claude => {
                config.claude_api_key = Some(api_key.clone());
                config.claude_endpoint = Some(endpoint.clone());
                config.claude_model = Some(model.clone());
            },
        }
        config.provider = Some(format!("{:?}", cli.provider));
        config.save()?;
    }
    
    // Get the diff
    let diff = if let Some(file_path) = cli.file {
        let mut file = fs::File::open(&file_path)
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
        content
    } else {
        get_diff_from_git(cli.commit.as_deref(), cli.range.as_deref())?
    };
    
    // Generate MR comment
    let mr_comment = generate_mr_comment(&diff, &api_key, &endpoint, &model, &cli.provider)?;
    
    // Output result
    if let Some(output_path) = cli.output {
        fs::write(&output_path, &mr_comment)
            .with_context(|| format!("Failed to write to file: {}", output_path.display()))?;
        println!("MR comment written to {}", output_path.display());
    } else {
        println!("\n--- Generated MR Comment ---\n");
        println!("{}", mr_comment);
        println!("\n---------------------------\n");
    }
    
    Ok(())
}
