//! Public HTTP retrieval with explicit access-control safeguards.

use anyhow::{bail, Context, Result};
use reqwest::{header, Client, StatusCode};
use std::time::Duration;
use tracing::warn;

const UA: &str = "mega-save/0.1 (+https://github.com/Keisei-Akiya/mega-save)";
const HTTP_MAX_ATTEMPTS: u32 = 4;
const HTTP_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

pub(crate) fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build HTTP client")
}

pub(crate) async fn get_public_text(
    client: &Client,
    url: &str,
    referer: Option<&str>,
) -> Result<String> {
    let response = public_request(client, url, referer).await?;
    response.text().await.context("read public work page body")
}

pub(crate) async fn get_public_bytes(
    client: &Client,
    url: &str,
    referer: Option<&str>,
) -> Result<Vec<u8>> {
    let response = public_request(client, url, referer).await?;
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.contains("text/html") {
        bail!("image request returned HTML; access may be blocked or require login: {url}");
    }
    response
        .bytes()
        .await
        .context("read public image body")
        .map(|b| b.to_vec())
}

async fn public_request(
    client: &Client,
    url: &str,
    referer: Option<&str>,
) -> Result<reqwest::Response> {
    for attempt in 1..=HTTP_MAX_ATTEMPTS {
        let mut request = client.get(url).header(
            header::ACCEPT,
            "text/html,application/xhtml+xml,image/avif,image/webp,image/png,image/jpeg,*/*;q=0.8",
        );
        if let Some(referer) = referer {
            request = request.header(header::REFERER, referer);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                if is_access_control_status(status) {
                    bail!("public access blocked for {url} (HTTP {status}); this command will not bypass login, DRM, or access controls");
                }
                if is_retryable_status(status) {
                    if attempt == HTTP_MAX_ATTEMPTS {
                        bail!("public GET {url}: retryable HTTP {status} persisted after {HTTP_MAX_ATTEMPTS} attempts");
                    }
                    let delay = retry_delay(attempt);
                    warn!(attempt, max_attempts = HTTP_MAX_ATTEMPTS, %status, delay_secs = delay.as_secs(), %url, "retrying transient public HTTP status");
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return response
                    .error_for_status()
                    .with_context(|| format!("public GET {url}"));
            }
            Err(error) if is_retryable_transport_error(&error) => {
                if attempt == HTTP_MAX_ATTEMPTS {
                    return Err(error).with_context(|| {
                        format!("public GET {url}: transient transport failure persisted after {HTTP_MAX_ATTEMPTS} attempts")
                    });
                }
                let delay = retry_delay(attempt);
                warn!(attempt, max_attempts = HTTP_MAX_ATTEMPTS, error = %error, delay_secs = delay.as_secs(), %url, "retrying transient public transport failure");
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error).with_context(|| format!("GET {url}")),
        }
    }
    unreachable!("retry loop always returns on its final attempt")
}

fn is_access_control_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    )
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

fn retry_delay(attempt: u32) -> Duration {
    HTTP_RETRY_BASE_DELAY.saturating_mul(2_u32.saturating_pow(attempt.saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_transient_http_statuses() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::from_u16(520).unwrap()));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(StatusCode::FORBIDDEN));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(is_access_control_status(StatusCode::UNAUTHORIZED));
        assert!(is_access_control_status(StatusCode::FORBIDDEN));
        assert!(is_access_control_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn retry_backoff_is_bounded_exponential() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(3), Duration::from_secs(4));
        assert_eq!(retry_delay(4), Duration::from_secs(8));
    }
}
