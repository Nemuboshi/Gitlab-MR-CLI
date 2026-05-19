# gitlab-mr-cli

`gitlab-mr-cli` is a standalone Rust CLI for working with GitLab merge request discussions through the legacy GitLab REST API.

## Features

- List all MR discussions
- List unresolved diff comments only
- Fetch MR diffs
- Read local file context around a commented line
- Reply to a discussion

## Configuration

`GITLAB_BASE_URL` and `GITLAB_TOKEN` are the only required settings. The merge request target is optional in config and can be passed per command as a single `--mr` value.

Configuration priority is:

1. CLI flags
2. `gitlab-mr-cli.toml`
3. Environment variables

Create a `gitlab-mr-cli.toml` file next to the binary or in your current working directory:

```toml
base_url = "http://gitlab.example.com"
token = "glpat-xxxxxxxxxxxxxxxxxxxx"

# Optional
mr = "group/project/-/merge_requests/123"
```

The CLI looks for `gitlab-mr-cli.toml` in this order:

1. The current working directory
2. The executable directory
3. The process working directory fallback path

## Usage

```bash
./gitlab-mr-cli unresolved --mr "group/project/-/merge_requests/123"
```

JSON output:

```bash
./gitlab-mr-cli unresolved --mr "group/project/-/merge_requests/123" --json
```

List raw discussions:

```bash
./gitlab-mr-cli discussions --mr "group/project/-/merge_requests/123"
```

Fetch diffs:

```bash
./gitlab-mr-cli diff --mr "group/project/-/merge_requests/123"
```

Read local file context:

```bash
./gitlab-mr-cli file-context \
  --repo-root "/path/to/local/repo" \
  --file "src/foo.ts" \
  --line 42 \
  --context-lines 12
```

Reply to a discussion:

```bash
./gitlab-mr-cli reply \
  --mr "group/project/-/merge_requests/123" \
  --discussion-id "abc123" \
  --note "Will fix this."
```

If `gitlab-mr-cli.toml` already contains `mr`, the command can be shorter:

```bash
./gitlab-mr-cli unresolved
```

If only `base_url` and `token` are stored in `gitlab-mr-cli.toml`, pass the MR target inline:

```bash
./gitlab-mr-cli unresolved --mr "xeus2.5/xeus-oms-pc-app/-/merge_requests/6"
```

Release archives always contain the same binary name:

- Linux/macOS: `gitlab-mr-cli`
- Windows: `gitlab-mr-cli.exe`

Platform and version only appear in the archive filename.

## Commands

### `unresolved`

List unresolved diff comments only.

```bash
./gitlab-mr-cli unresolved --mr "xeus2.5/xeus-oms-pc-app/-/merge_requests/6" --json
```

### `discussions`

List raw MR discussions as JSON.

```bash
./gitlab-mr-cli discussions --mr "group/project/-/merge_requests/123"
```

### `diff`

Fetch MR file diffs from the legacy GitLab `changes` endpoint.

```bash
./gitlab-mr-cli diff --mr "group/project/-/merge_requests/123"
```

### `file-context`

Read local repository context around a target line.

```bash
./gitlab-mr-cli file-context \
  --repo-root "/path/to/repo" \
  --file "src/foo.ts" \
  --line 42
```

### `reply`

Reply to an existing discussion.

```bash
./gitlab-mr-cli reply \
  --mr "group/project/-/merge_requests/123" \
  --discussion-id "discussion-id" \
  --note "Updated."
```

## Skill template

Use the following template if you want an AI tool to automatically prefer this CLI when GitLab review work is mentioned.

```md
# GitLab MR Review Skill

Use this skill when the user mentions GitLab review workflows, including keywords such as:

- GitLab
- MR
- merge request
- PR
- pull request
- discussion
- review comment
- unresolved comment
- unresolved thread
- reply to comment
- review feedback

Prefer this skill when the user is trying to inspect, summarize, filter, or reply to GitLab MR discussions.

## Tooling

This skill uses the local `gitlab-mr-cli` binary.

## Suggested commands

List unresolved comments:

```bash
gitlab-mr-cli unresolved --mr "<group/project/-/merge_requests/iid>" --json
```

List all discussions:

```bash
gitlab-mr-cli discussions --mr "<group/project/-/merge_requests/iid>" --json
```

Get diffs:

```bash
gitlab-mr-cli diff --mr "<group/project/-/merge_requests/iid>" --json
```

Read local file context:

```bash
gitlab-mr-cli file-context --repo-root "<repo-root>" --file "<path>" --line <line> --json
```

Reply to a discussion:

```bash
gitlab-mr-cli reply --mr "<group/project/-/merge_requests/iid>" --discussion-id "<discussion-id>" --note "<reply>"
```

## Guidance

- Prefer `unresolved` when the user asks about open review work.
- Prefer `discussions` when the user wants raw thread detail.
- Prefer `diff` when the user wants review context.
- Prefer `file-context` when the repository is available locally and line-level context is needed.
- Use `reply` only when the user clearly wants to send a message back to GitLab.
```
