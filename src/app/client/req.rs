use anyhow::Result;
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use reqwest::{
    Client, Method,
    header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, USER_AGENT},
};

use super::sessions::load_session;

pub struct PreRequest {
    pub cookie: Vec<String>,
    pub method: String,
    pub url: String,
    pub body: String,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
    pub http_version: Option<String>,
    pub custom_headers: Option<std::collections::HashMap<String, String>>,
}

pub async fn make_req(prereq: PreRequest) -> Result<reqwest::Response> {
    let client = if let Some(proxy_url) = prereq.proxy {
        Client::builder()
            .proxy(reqwest::Proxy::all(proxy_url)?)
            .build()?
    } else {
        Client::new()
    };

    let method = match prereq.method.to_uppercase().as_str() {
        "POST" => Method::POST,
        "GET" => Method::GET,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        "PUT" => Method::PUT,
        "OPTIONS" => Method::OPTIONS,
        _ => Method::GET,
    };

    let mut req_builder = client.request(method, &prereq.url);

    if let Some(v) = &prereq.http_version {
        match v.as_str() {
            "1.0" | "HTTP/1.0" => req_builder = req_builder.version(reqwest::Version::HTTP_10),
            "1.1" | "HTTP/1.1" => req_builder = req_builder.version(reqwest::Version::HTTP_11),
            "2" | "2.0" | "HTTP/2" | "HTTP/2.0" => {
                req_builder = req_builder.version(reqwest::Version::HTTP_2)
            }
            "3" | "3.0" | "HTTP/3" | "HTTP/3.0" => {
                req_builder = req_builder.version(reqwest::Version::HTTP_3)
            }
            _ => {}
        }
    }

    if !prereq.body.is_empty() {
        req_builder = req_builder.body(prereq.body);
    }

    let mut headers = HeaderMap::new();

    if let Some(custom) = prereq.custom_headers {
        for (k, v) in custom {
            use std::str::FromStr;
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_str(&k),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                headers.insert(name, val);
            }
        }
    }

    let session = load_session().await?;

    let mut all_cookies = session.cookie.clone();
    all_cookies.extend(prereq.cookie);

    if !all_cookies.is_empty() {
        let cookie_str = all_cookies.join("; ");
        if let Ok(val) = HeaderValue::from_str(&cookie_str) {
            headers.insert(COOKIE, val);
        }
    }

    if let Some(token) = session.auth_token {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
            headers.insert(AUTHORIZATION, val);
        }
    }

    if let Some(ua) = prereq.user_agent {
        if let Ok(val) = HeaderValue::from_str(&ua) {
            headers.insert(USER_AGENT, val);
        }
    } else {
        if let Ok(val) = HeaderValue::from_str(random_user_agent()) {
            headers.insert(USER_AGENT, val);
        }
    }

    req_builder = req_builder.headers(headers);

    let resp = req_builder.send().await?;
    Ok(resp)
}

pub fn random_user_agent() -> &'static str {
    let agents = random_user_agents();
    agents.choose(&mut rand::rng()).unwrap()
}

pub fn random_user_agents() -> Vec<&'static str> {
    vec![
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4_1) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Safari/605.1.15",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36 Edg/123.0.0.0",
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.6367.82 Mobile Safari/537.36",
        "Mozilla/5.0 (iPad; CPU OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 OPR/110.0.0.0",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Linux; Android 13; Samsung SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.6367.82 Mobile Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:125.0) Gecko/20100101 Firefox/125.0",
    ]
}
