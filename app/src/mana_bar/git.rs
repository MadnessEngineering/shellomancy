//! The focused repo's recent commits, for the Mana Bar's Log: read with
//! `git log` in the background, mixed in with the todo activity, and linked
//! to the todos whose completion notes name them.

use std::path::PathBuf;

/// Commits read per refresh.
const LOG_LIMIT: &str = "50";
/// Between a commit's fields, and after each commit, in `git log` output.
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';
/// The shortest hex run that counts as naming a commit.
const MIN_SHA_LEN: usize = 7;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub author: String,
    /// Unix seconds (committer time).
    pub at: i64,
    pub subject: String,
    pub body: String,
}

impl Commit {
    /// The sha as our notes write it.
    pub fn short(&self) -> &str {
        self.sha.get(..8).unwrap_or(&self.sha)
    }
}

/// A repo's recent commits and where they can be seen online.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepoCommits {
    pub root: PathBuf,
    pub commits: Vec<Commit>,
    /// `https://github.com/<owner>/<repo>` when `origin` is on GitHub.
    pub web: Option<String>,
}

/// `git log` arguments whose output [`parse_log`] reads.
pub fn log_args() -> [&'static str; 4] {
    [
        "log",
        "-n",
        LOG_LIMIT,
        "--format=%H%x1f%an%x1f%ct%x1f%s%x1f%b%x1e",
    ]
}

pub fn parse_log(output: &str) -> Vec<Commit> {
    output
        .split(RECORD)
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').splitn(5, FIELD);
            let sha = fields.next()?.trim();
            if sha.len() < MIN_SHA_LEN || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Some(Commit {
                sha: sha.to_owned(),
                author: fields.next().unwrap_or_default().trim().to_owned(),
                at: fields
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .unwrap_or(0),
                subject: fields.next().unwrap_or_default().trim().to_owned(),
                body: fields.next().unwrap_or_default().trim().to_owned(),
            })
        })
        .collect()
}

/// A GitHub repo's web address from its remote URL (ssh or https).
pub fn github_web(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("http://github.com/"))?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none())
        .then(|| format!("https://github.com/{owner}/{repo}"))
}

/// Whether `text` names the commit: a run of 7+ hex digits, standing alone,
/// that the sha starts with.
pub fn mentions(text: &str, sha: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= MIN_SHA_LEN && word.chars().all(|c| c.is_ascii_hexdigit()))
        .any(|word| sha.starts_with(&word.to_ascii_lowercase()))
}

pub async fn fetch_commits(root: PathBuf) -> Result<RepoCommits, String> {
    let output = warp_util::git::run_git_command(&root, &log_args())
        .await
        .map_err(|err| err.to_string())?;
    let web = warp_util::git::run_git_command(&root, &["remote", "get-url", "origin"])
        .await
        .ok()
        .and_then(|remote| github_web(&remote));
    Ok(RepoCommits {
        commits: parse_log(&output),
        web,
        root,
    })
}

/// The files a commit changed, as `git show --stat` prints them.
pub async fn fetch_files(root: PathBuf, sha: String) -> Result<String, String> {
    warp_util::git::run_git_command(&root, &files_args(&sha))
        .await
        .map(|stat| stat.trim().to_owned())
        .map_err(|err| err.to_string())
}

fn files_args(sha: &str) -> [&str; 4] {
    ["show", "--stat", "--format=", sha]
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
