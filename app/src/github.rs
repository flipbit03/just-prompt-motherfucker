//! The GitHub half of signing. Not a login: the token is used once to read
//! the account id, then dropped. No session is created.

use reqwest::Url;
use serde::Deserialize;

const AUTHORIZE: &str = "https://github.com/login/oauth/authorize";
const TOKEN: &str = "https://github.com/login/oauth/access_token";
const USER: &str = "https://api.github.com/user";

/// GitHub rejects API requests that arrive without a User-Agent.
const UA: &str = concat!("jpmf/", env!("CARGO_PKG_VERSION"));

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
pub struct Oauth {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    http: reqwest::Client,
}

/// `id` is the identity we key on: handles get renamed and recycled, ids do
/// not. The avatar URL derives from it, so nothing else is worth storing.
#[derive(Deserialize)]
pub struct User {
    pub id: i64,
    pub login: String,
}

/// GitHub answers a bad code with HTTP 200 and an error in the body.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl Oauth {
    pub fn new(client_id: String, client_secret: String, base_url: &str) -> Result<Self, Error> {
        Ok(Self {
            client_id,
            client_secret,
            // Must match a callback URL registered on the OAuth App exactly.
            redirect_uri: format!("{base_url}/auth/callback"),
            http: reqwest::Client::builder().user_agent(UA).build()?,
        })
    }

    /// `scope` is empty on purpose: with no scopes the consent screen says
    /// this app reads public profile information and nothing else.
    pub fn authorize_url(&self, state: &str) -> String {
        Url::parse_with_params(
            AUTHORIZE,
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("scope", ""),
                ("state", state),
            ],
        )
        .expect("authorize endpoint is a constant and every parameter is a string")
        .into()
    }

    /// Trade the one-time code for a token. Codes are single-use and expire in
    /// about ten minutes, so failure here is often just a slow user.
    pub async fn exchange(&self, code: &str) -> Result<String, Error> {
        let body: TokenResponse = self
            .http
            .post(TOKEN)
            // Without this GitHub replies form-encoded rather than JSON.
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
            ])
            .send()
            .await?
            .json()
            .await?;

        body.access_token.ok_or_else(|| {
            format!(
                "github refused the code: {} ({})",
                body.error.as_deref().unwrap_or("unknown"),
                body.error_description.as_deref().unwrap_or("no detail"),
            )
            .into()
        })
    }

    /// Ask who the token belongs to.
    pub async fn user(&self, token: &str) -> Result<User, Error> {
        let res = self.http.get(USER).bearer_auth(token).send().await?;
        let status = res.status();
        if !status.is_success() {
            return Err(format!("github /user returned {status}").into());
        }
        Ok(res.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth() -> Oauth {
        Oauth::new("cid".into(), "secret".into(), "http://localhost:8100").unwrap()
    }

    #[test]
    fn authorize_url_has_no_scopes() {
        let url = oauth().authorize_url("abc.sign");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("scope=&") || url.ends_with("scope="));
        assert!(url.contains("state=abc.sign"));
    }

    #[test]
    fn redirect_uri_is_percent_encoded() {
        // Built through Url rather than format!, so the callback's :// survives
        // being a query parameter.
        let url = oauth().authorize_url("s");
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8100%2Fauth%2Fcallback"));
    }

    #[test]
    fn the_secret_never_appears_in_the_authorize_url() {
        assert!(!oauth().authorize_url("s").contains("secret"));
    }
}
