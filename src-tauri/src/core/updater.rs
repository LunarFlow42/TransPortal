//! Update checker for TransPortal querying GitHub Releases API.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub release_url: String,
    pub download_url: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    assets: Option<Vec<GithubAsset>>,
}

pub fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => latest.trim_start_matches('v') != current.trim_start_matches('v'),
    }
}

pub async fn check_github_update(current_version: &str) -> Result<UpdateCheckResult, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("TransPortal/{}", current_version))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建网络请求客户端失败: {e}"))?;

    let res = client
        .get("https://api.github.com/repos/LunarFlow42/TransPortal/releases/latest")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("无法连接至 GitHub Releases 服务器: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("GitHub API 响应异常: HTTP {}", res.status()));
    }

    let release = res
        .json::<GithubRelease>()
        .await
        .map_err(|e| format!("解析 GitHub Release 数据失败: {e}"))?;

    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer_version(current_version, &release.tag_name);

    let download_url = release.assets.as_ref().and_then(|assets| {
        assets
            .iter()
            .find(|a| a.name.ends_with(".exe") || a.name.ends_with(".msi"))
            .map(|a| a.browser_download_url.clone())
    });

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version,
        has_update,
        release_name: release.name,
        release_notes: release.body,
        release_url: release.html_url,
        download_url,
        published_at: release.published_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parsing() {
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v1.2.3-alpha.1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("invalid"), None);
    }

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));
        assert!(!is_newer_version("0.1.1", "0.1.0"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
    }
}
