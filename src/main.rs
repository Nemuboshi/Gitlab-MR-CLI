use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs;

#[derive(Parser)]
#[command(name = "gitlab-mr-cli")]
#[command(about = "CLI helper for GitLab MR discussions via the legacy REST API.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Unresolved(CommonArgsWithJson),
    Discussions(CommonArgsWithJson),
    Diff(CommonArgsWithJson),
    FileContext(FileContextArgs),
    Reply(ReplyArgs),
}

#[derive(Parser, Clone)]
struct CommonArgsWithJson {
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(
        long,
        help = "Merge request reference, for example group/project/-/merge_requests/123"
    )]
    mr: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Parser)]
struct FileContextArgs {
    #[arg(long)]
    repo_root: String,
    #[arg(long)]
    file: String,
    #[arg(long)]
    line: usize,
    #[arg(long, default_value_t = 10)]
    context_lines: usize,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Parser)]
struct ReplyArgs {
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(
        long,
        help = "Merge request reference, for example group/project/-/merge_requests/123"
    )]
    mr: Option<String>,
    #[arg(long)]
    discussion_id: String,
    #[arg(long)]
    note: String,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Clone)]
struct GitLabClientConfig {
    base_url: String,
    token: String,
}

#[derive(Clone, Default, Deserialize)]
struct FileConfig {
    base_url: Option<String>,
    token: Option<String>,
    mr: Option<String>,
}

#[derive(Clone)]
struct ResolvedMrTarget {
    project: String,
    mr_iid: u64,
}

#[derive(Deserialize, Serialize)]
struct GitLabDiscussion {
    id: String,
    notes: Vec<GitLabDiscussionNote>,
}

#[derive(Deserialize, Serialize)]
struct GitLabDiscussionNote {
    id: u64,
    #[serde(rename = "type")]
    note_type: Option<String>,
    body: String,
    system: bool,
    #[serde(default)]
    resolvable: bool,
    #[serde(default)]
    resolved: bool,
    created_at: String,
    author: Option<GitLabAuthor>,
    position: Option<GitLabDiscussionPosition>,
}

#[derive(Deserialize, Serialize)]
struct GitLabAuthor {
    username: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct GitLabDiscussionPosition {
    old_path: Option<String>,
    new_path: Option<String>,
    old_line: Option<u64>,
    new_line: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MrComment {
    discussion_id: String,
    note_id: u64,
    body: String,
    file: Option<String>,
    line: Option<u64>,
    author: String,
    created_at: String,
    resolved: bool,
}

#[derive(Deserialize)]
struct GitLabChangesResponse {
    changes: Vec<GitLabChange>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitLabChange {
    old_path: String,
    new_path: String,
    diff: String,
    new_file: bool,
    renamed_file: bool,
    deleted_file: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReplyResult {
    discussion_id: String,
    note_id: u64,
    body: String,
    created_at: String,
    author: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileContext {
    file_path: String,
    start_line: usize,
    end_line: usize,
    target_line: usize,
    lines: Vec<FileContextLine>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileContextLine {
    line_number: usize,
    content: String,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let file_config = load_file_config()?;
    let client = Client::builder()
        .build()
        .context("Failed to create HTTP client")?;

    match cli.command {
        Commands::Unresolved(args) => {
            let (config, project, mr_iid) = resolve_common_args(&args, &file_config)?;
            let comments = list_unresolved_comments(&client, &config, &project, mr_iid).await?;
            if args.json {
                print_json(&comments)?;
            } else {
                print_unresolved_comments(&comments);
            }
        }
        Commands::Discussions(args) => {
            let (config, project, mr_iid) = resolve_common_args(&args, &file_config)?;
            let discussions =
                list_merge_request_discussions(&client, &config, &project, mr_iid).await?;
            if !args.json {
                println!("Found {} discussions.", discussions.len());
            }
            print_json(&discussions)?;
        }
        Commands::Diff(args) => {
            let (config, project, mr_iid) = resolve_common_args(&args, &file_config)?;
            let changes = get_merge_request_diff(&client, &config, &project, mr_iid).await?;
            if args.json {
                print_json(&changes)?;
            } else {
                print_diff(&changes);
            }
        }
        Commands::FileContext(args) => {
            let context = get_file_around_comment(
                Path::new(&args.repo_root),
                &args.file,
                args.line,
                args.context_lines,
            )
            .await?;
            if args.json {
                print_json(&context)?;
            } else {
                print_file_context(&context);
            }
        }
        Commands::Reply(args) => {
            let common = CommonArgsWithJson {
                base_url: args.base_url.clone(),
                token: args.token.clone(),
                mr: args.mr.clone(),
                json: args.json,
            };
            let (config, project, mr_iid) = resolve_common_args(&common, &file_config)?;
            let result = reply_to_discussion(
                &client,
                &config,
                &project,
                mr_iid,
                &args.discussion_id,
                &args.note,
            )
            .await?;
            if args.json {
                print_json(&result)?;
            } else {
                println!(
                    "Replied to discussion {} as note {}.",
                    result.discussion_id, result.note_id
                );
            }
        }
    }

    Ok(())
}

fn load_file_config() -> Result<FileConfig> {
    let current_dir = env::current_dir().context("Failed to read current directory")?;
    let current_exe = env::current_exe().context("Failed to read current executable path")?;
    let exe_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));

    let candidates = [
        current_dir.join("gitlab-mr-cli.toml"),
        exe_dir.join("gitlab-mr-cli.toml"),
        PathBuf::from("gitlab-mr-cli.toml"),
    ];

    for path in candidates {
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let config: FileConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            return Ok(config);
        }
    }

    Ok(FileConfig::default())
}

fn resolve_string(
    cli_value: &Option<String>,
    file_value: &Option<String>,
    env_name: &str,
) -> Option<String> {
    cli_value
        .clone()
        .or_else(|| file_value.clone())
        .or_else(|| env::var(env_name).ok())
}

fn require_value(value: Option<String>, message: &str) -> Result<String> {
    value.ok_or_else(|| anyhow!(message.to_string()))
}

fn resolve_common_args(
    args: &CommonArgsWithJson,
    file_config: &FileConfig,
) -> Result<(GitLabClientConfig, String, u64)> {
    let base_url = require_value(
        resolve_string(&args.base_url, &file_config.base_url, "GITLAB_BASE_URL"),
        "Missing GitLab base URL. Pass --base-url, set it in gitlab-mr-cli.toml, or set GITLAB_BASE_URL.",
    )?;
    let token = require_value(
        resolve_string(&args.token, &file_config.token, "GITLAB_TOKEN"),
        "Missing GitLab token. Pass --token, set it in gitlab-mr-cli.toml, or set GITLAB_TOKEN.",
    )?;

    let mr_target = resolve_mr_target(args, file_config)?;

    Ok((
        GitLabClientConfig { base_url, token },
        mr_target.project,
        mr_target.mr_iid,
    ))
}

fn resolve_mr_target(
    args: &CommonArgsWithJson,
    file_config: &FileConfig,
) -> Result<ResolvedMrTarget> {
    let mr_reference = resolve_string(&args.mr, &file_config.mr, "GITLAB_MR");

    match mr_reference {
        Some(reference) => parse_mr_reference(&reference),
        None => bail!(
            "Missing merge request target. Pass --mr, set it in gitlab-mr-cli.toml, or set GITLAB_MR."
        ),
    }
}

fn parse_mr_reference(reference: &str) -> Result<ResolvedMrTarget> {
    let normalized = reference.trim().trim_matches('/');
    let marker = "/-/merge_requests/";
    let (project, mr_raw) = normalized
        .split_once(marker)
        .ok_or_else(|| anyhow!("Invalid MR reference: {reference}. Expected format like group/project/-/merge_requests/123"))?;

    if project.is_empty() {
        bail!("Invalid MR reference: missing project path in {reference}");
    }

    Ok(ResolvedMrTarget {
        project: project.to_string(),
        mr_iid: parse_mr_iid(mr_raw)?,
    })
}

fn parse_mr_iid(mr_raw: &str) -> Result<u64> {
    let mr_iid = mr_raw
        .parse::<u64>()
        .with_context(|| format!("MR IID must be a positive integer, got: {mr_raw}"))?;

    if mr_iid == 0 {
        bail!("MR IID must be a positive integer, got: {mr_raw}");
    }

    Ok(mr_iid)
}

fn normalize_base_url(base_url: &str) -> &str {
    base_url.trim_end_matches('/')
}

fn build_api_url(
    config: &GitLabClientConfig,
    endpoint: &str,
    query: &[(&str, String)],
) -> Result<String> {
    let mut url = reqwest::Url::parse(&format!(
        "{}/api/v4{}",
        normalize_base_url(&config.base_url),
        endpoint
    ))
    .with_context(|| format!("Failed to build API URL for endpoint {endpoint}"))?;

    for (key, value) in query {
        url.query_pairs_mut().append_pair(key, value);
    }

    Ok(url.to_string())
}

fn project_ref(project: &str) -> String {
    urlencoding::encode(project).to_string()
}

fn default_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("PRIVATE-TOKEN", HeaderValue::from_str(token)?);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

async fn gitlab_fetch<T: for<'de> Deserialize<'de>>(
    client: &Client,
    config: &GitLabClientConfig,
    endpoint: &str,
    query: &[(&str, String)],
    method: reqwest::Method,
    body: Option<serde_json::Value>,
) -> Result<T> {
    let url = build_api_url(config, endpoint, query)?;
    let mut request = client
        .request(method, url)
        .headers(default_headers(&config.token)?);

    if let Some(value) = body {
        request = request.json(&value);
    }

    let response = request.send().await?;
    let status = response.status();

    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        bail!(
            "GitLab API failed for {endpoint}: {status}{}",
            if error_body.is_empty() {
                String::new()
            } else {
                format!("\n{error_body}")
            }
        );
    }

    Ok(response.json::<T>().await?)
}

async fn gitlab_fetch_all_pages<T: for<'de> Deserialize<'de>>(
    client: &Client,
    config: &GitLabClientConfig,
    endpoint: &str,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    let mut page = 1_u64;

    loop {
        let url = build_api_url(
            config,
            endpoint,
            &[("page", page.to_string()), ("per_page", "100".to_string())],
        )?;
        let response = client
            .get(url)
            .headers(default_headers(&config.token)?)
            .send()
            .await?;
        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            bail!(
                "GitLab API failed for {endpoint}: {status}{}",
                if error_body.is_empty() {
                    String::new()
                } else {
                    format!("\n{error_body}")
                }
            );
        }

        let next_page = response
            .headers()
            .get("x-next-page")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        let mut page_items = response.json::<Vec<T>>().await?;
        items.append(&mut page_items);

        match next_page.as_deref() {
            Some("") | None => break,
            Some(value) => {
                page = value.parse::<u64>().unwrap_or(0);
                if page == 0 {
                    break;
                }
            }
        }
    }

    Ok(items)
}

fn get_author_name(note: &GitLabDiscussionNote) -> String {
    note.author
        .as_ref()
        .and_then(|author| author.username.clone().or(author.name.clone()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn get_note_file(note: &GitLabDiscussionNote) -> Option<String> {
    note.position
        .as_ref()
        .and_then(|position| position.new_path.clone().or(position.old_path.clone()))
}

fn get_note_line(note: &GitLabDiscussionNote) -> Option<u64> {
    note.position
        .as_ref()
        .and_then(|position| position.new_line.or(position.old_line))
}

fn is_unresolved_diff_note(note: &GitLabDiscussionNote) -> bool {
    note.note_type.as_deref() == Some("DiffNote")
        && !note.system
        && note.resolvable
        && !note.resolved
}

async fn list_merge_request_discussions(
    client: &Client,
    config: &GitLabClientConfig,
    project: &str,
    mr_iid: u64,
) -> Result<Vec<GitLabDiscussion>> {
    gitlab_fetch_all_pages(
        client,
        config,
        &format!(
            "/projects/{}/merge_requests/{mr_iid}/discussions",
            project_ref(project)
        ),
    )
    .await
}

async fn list_unresolved_comments(
    client: &Client,
    config: &GitLabClientConfig,
    project: &str,
    mr_iid: u64,
) -> Result<Vec<MrComment>> {
    let discussions = list_merge_request_discussions(client, config, project, mr_iid).await?;

    Ok(discussions
        .into_iter()
        .flat_map(|discussion| {
            let discussion_id = discussion.id.clone();
            discussion
                .notes
                .into_iter()
                .filter(is_unresolved_diff_note)
                .map(move |note| {
                    let file = get_note_file(&note);
                    let line = get_note_line(&note);
                    let author = get_author_name(&note);

                    MrComment {
                        discussion_id: discussion_id.clone(),
                        note_id: note.id,
                        body: note.body,
                        file,
                        line,
                        author,
                        created_at: note.created_at,
                        resolved: note.resolved,
                    }
                })
        })
        .collect())
}

async fn get_merge_request_diff(
    client: &Client,
    config: &GitLabClientConfig,
    project: &str,
    mr_iid: u64,
) -> Result<Vec<GitLabChange>> {
    let result: GitLabChangesResponse = gitlab_fetch(
        client,
        config,
        &format!(
            "/projects/{}/merge_requests/{mr_iid}/changes",
            project_ref(project)
        ),
        &[],
        reqwest::Method::GET,
        None,
    )
    .await?;

    Ok(result.changes)
}

async fn reply_to_discussion(
    client: &Client,
    config: &GitLabClientConfig,
    project: &str,
    mr_iid: u64,
    discussion_id: &str,
    note: &str,
) -> Result<ReplyResult> {
    let result: GitLabDiscussion = gitlab_fetch(
        client,
        config,
        &format!(
            "/projects/{}/merge_requests/{mr_iid}/discussions/{}/notes",
            project_ref(project),
            urlencoding::encode(discussion_id)
        ),
        &[],
        reqwest::Method::POST,
        Some(json!({ "body": note })),
    )
    .await?;

    let created_note = result
        .notes
        .last()
        .ok_or_else(|| anyhow!("GitLab API returned no note after creating a reply."))?;

    Ok(ReplyResult {
        discussion_id: result.id,
        note_id: created_note.id,
        body: created_note.body.clone(),
        created_at: created_note.created_at.clone(),
        author: get_author_name(created_note),
    })
}

async fn get_file_around_comment(
    repo_root: &Path,
    file_path: &str,
    line: usize,
    context_lines: usize,
) -> Result<FileContext> {
    let absolute_path = repo_root.join(file_path);
    let content = fs::read_to_string(&absolute_path)
        .await
        .with_context(|| format!("Failed to read {}", absolute_path.display()))?;
    let lines: Vec<&str> = content.lines().collect();

    if line == 0 || line > lines.len() {
        bail!(
            "Line {line} is out of range for {file_path} ({} lines).",
            lines.len()
        );
    }

    let start_line = line.saturating_sub(context_lines).max(1);
    let end_line = (line + context_lines).min(lines.len());
    let excerpt = (start_line..=end_line)
        .map(|line_number| FileContextLine {
            line_number,
            content: lines[line_number - 1].to_string(),
        })
        .collect();

    Ok(FileContext {
        file_path: file_path.to_string(),
        start_line,
        end_line,
        target_line: line,
        lines: excerpt,
    })
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_unresolved_comments(comments: &[MrComment]) {
    if comments.is_empty() {
        println!("No unresolved diff comments found.");
        return;
    }

    for comment in comments {
        println!("{}", "=".repeat(80));
        println!("file: {}", comment.file.as_deref().unwrap_or("(unknown)"));
        println!(
            "line: {}",
            comment
                .line
                .map(|line| line.to_string())
                .unwrap_or_else(|| "(unknown)".to_string())
        );
        println!("author: {}", comment.author);
        println!("discussion: {}", comment.discussion_id);
        println!("note: {}", comment.note_id);
        println!("created: {}", comment.created_at);
        println!();
        println!("{}", comment.body);
        println!();
    }
}

fn print_diff(changes: &[GitLabChange]) {
    if changes.is_empty() {
        println!("No changes found.");
        return;
    }

    for change in changes {
        println!("{}", "=".repeat(80));
        println!("{} -> {}", change.old_path, change.new_path);
        println!(
            "new={} renamed={} deleted={}",
            change.new_file, change.renamed_file, change.deleted_file
        );
        println!();
        println!("{}", change.diff);
        println!();
    }
}

fn print_file_context(context: &FileContext) {
    println!(
        "{}:{}-{} (target {})",
        context.file_path, context.start_line, context.end_line, context.target_line
    );

    for line in &context.lines {
        let marker = if line.line_number == context.target_line {
            ">"
        } else {
            " "
        };
        println!("{marker} {:>5} | {}", line.line_number, line.content);
    }
}
