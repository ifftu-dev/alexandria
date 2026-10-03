pub mod evaluation;
pub mod learning;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alexandria_learning_contracts::{Judgment, Mode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Semaphore;

pub const MODEL: &str = "jev-1.13.0";
pub const MAX_REQUEST_BYTES: usize = 32_000;
pub const MAX_QUESTIONS: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    Noul {
        instructions: Value,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
}

impl Question {
    pub fn choice(instructions: impl Into<String>, options: &[&str]) -> Self {
        Self::Choice {
            instructions: Value::String(instructions.into()),
            criteria: options
                .iter()
                .map(|o| ((*o).to_owned(), Value::String((*o).to_owned())))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Noul {
        noul: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

impl Answer {
    pub fn judgment(&self) -> Result<Judgment, Error> {
        match self {
            Self::Choice {
                choice,
                probabilities,
                confidence,
            } => Ok(Judgment {
                value: choice.clone(),
                probabilities: probabilities.clone(),
                confidence: *confidence,
            }),
            _ => Err(Error::Invalid),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}

impl Request {
    pub fn new(state: Value, questions: BTreeMap<String, Question>) -> Self {
        Self {
            model: MODEL.into(),
            state,
            questions,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        if self.questions.is_empty() || self.questions.len() > MAX_QUESTIONS || self.model != MODEL
        {
            return Err(Error::Invalid);
        }
        for q in self.questions.values() {
            if let Question::Score { criteria, .. } = q {
                if !(2..=10).contains(&criteria.len()) {
                    return Err(Error::Invalid);
                }
            }
            if let Question::Choice { criteria, .. } = q {
                if !(2..=255).contains(&criteria.len()) {
                    return Err(Error::Invalid);
                }
            }
        }
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(bytes)
    }
    pub fn validate(&self, response: &Response) -> Result<(), Error> {
        if response.model != self.model || response.answers.len() != self.questions.len() {
            return Err(Error::Invalid);
        }
        for (id, question) in &self.questions {
            match (question, response.answers.get(id)) {
                (Question::Choice { criteria, .. }, Some(answer @ Answer::Choice { .. })) => {
                    let options: Vec<_> = criteria.keys().map(String::as_str).collect();
                    answer
                        .judgment()?
                        .validate(&options)
                        .map_err(|_| Error::Invalid)?;
                }
                (Question::Noul { .. }, Some(Answer::Noul { noul }))
                    if noul.is_finite() && (0.0..=1.0).contains(noul) => {}
                (
                    Question::Score { criteria, .. },
                    Some(Answer::Score {
                        score,
                        legend,
                        probabilities,
                        confidence,
                    }),
                ) => {
                    let keys: Vec<_> = (0..criteria.len()).map(|n| n.to_string()).collect();
                    if !score.is_finite()
                        || !confidence.is_finite()
                        || !(0.0..=1.0).contains(confidence)
                        || legend.len() != keys.len()
                        || probabilities.len() != keys.len()
                        || keys.iter().any(|key| {
                            !legend.contains_key(key)
                                || !probabilities
                                    .get(key)
                                    .is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(p))
                        })
                        || (probabilities.values().sum::<f64>() - 1.0).abs() > 0.001
                    {
                        return Err(Error::Invalid);
                    }
                    let expected: f64 = keys
                        .iter()
                        .enumerate()
                        .map(|(i, key)| i as f64 * probabilities[key])
                        .sum();
                    if (score - expected).abs() > 0.001 {
                        return Err(Error::Invalid);
                    }
                }
                _ => return Err(Error::Invalid),
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Decision provider is disabled or cloud processing is not allowed")]
    Disabled,
    #[error("Invalid decision configuration or response")]
    Invalid,
    #[error("Decision context exceeds the size limit")]
    TooLarge,
    #[error("Decision budget or concurrency limit reached")]
    Budget,
    #[error("Decision provider did not respond within the deadline")]
    Timeout,
    #[error("Decision provider is unavailable")]
    Unavailable,
}

#[derive(Clone)]
pub struct Config {
    pub mode: Mode,
    pub api_key: String,
    pub endpoint: String,
    pub cloud_allowed: bool,
    pub assist_approved: bool,
    pub hourly_token_budget: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            api_key: String::new(),
            endpoint: "https://api.typesafe.ai/v1/systemone".into(),
            cloud_allowed: false,
            assist_approved: false,
            hourly_token_budget: 200_000,
        }
    }
}

impl Config {
    pub fn from_env(prefix: &str) -> Result<Self, Error> {
        let read = |key: &str| std::env::var(format!("{prefix}_{key}")).unwrap_or_default();
        let mode = match read("MODE").as_str() {
            "" | "off" => Mode::Off,
            "shadow" => Mode::Shadow,
            "assist" => Mode::Assist,
            _ => return Err(Error::Invalid),
        };
        Ok(Self {
            mode,
            api_key: read("API_KEY"),
            cloud_allowed: read("CLOUD_ALLOWED") == "1",
            assist_approved: read("ASSIST_APPROVED") == "1",
            ..Self::default()
        })
    }
}

struct Budget {
    since: Instant,
    reserved: usize,
}

#[derive(Clone)]
pub struct Client {
    config: Config,
    http: reqwest::Client,
    slots: Arc<Semaphore>,
    budgets: Arc<Mutex<HashMap<String, Budget>>>,
}

impl Client {
    pub fn new(config: Config) -> Result<Self, Error> {
        let url = reqwest::Url::parse(&config.endpoint).map_err(|_| Error::Invalid)?;
        let loopback = url
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]"));
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || (url.scheme() != "https" && !(loopback && url.scheme() == "http"))
        {
            return Err(Error::Invalid);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            config,
            http,
            slots: Arc::new(Semaphore::new(4)),
            budgets: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    pub fn mode(&self) -> Mode {
        self.config.mode
    }
    pub fn enabled(&self) -> bool {
        self.config.mode != Mode::Off
            && self.config.cloud_allowed
            && !self.config.api_key.is_empty()
            && (self.config.mode != Mode::Assist || self.config.assist_approved)
    }
    pub async fn evaluate(&self, scope: &str, request: &Request) -> Result<Response, Error> {
        if !self.enabled() {
            return Err(Error::Disabled);
        }
        if scope.is_empty() || scope.len() > 200 {
            return Err(Error::Invalid);
        }
        let bytes = request.encode()?;
        let _permit = self.slots.try_acquire().map_err(|_| Error::Budget)?;
        {
            let mut budgets = self.budgets.lock().map_err(|_| Error::Unavailable)?;
            budgets.retain(|_, b| b.since.elapsed() < Duration::from_secs(3600));
            if !budgets.contains_key(scope) && budgets.len() >= 4096 {
                return Err(Error::Budget);
            }
            let budget = budgets.entry(scope.to_owned()).or_insert(Budget {
                since: Instant::now(),
                reserved: 0,
            });
            let reserve = bytes.len().saturating_add(1024);
            if budget.reserved.saturating_add(reserve) > self.config.hourly_token_budget {
                return Err(Error::Budget);
            }
            budget.reserved += reserve;
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut response = self
                .http
                .post(&self.config.endpoint)
                .bearer_auth(&self.config.api_key)
                .header("Content-Type", "application/json")
                .body(bytes)
                .send()
                .await
                .map_err(|_| Error::Unavailable)?;
            if !response.status().is_success() {
                return Err(Error::Unavailable);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
                if body.len() + chunk.len() > 1_000_000 {
                    return Err(Error::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            let result: Response = serde_json::from_slice(&body).map_err(|_| Error::Invalid)?;
            request.validate(&result)?;
            Ok(result)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> Request {
        Request::new(
            json!("data"),
            BTreeMap::from([("q".into(), Question::choice("Supported?", &["yes", "no"]))]),
        )
    }
    #[test]
    fn rejects_wrong_model_partial_and_invented_answers() {
        let mut r: Response = serde_json::from_value(json!({"model":MODEL,"usage":{"input_tokens":10,"output_tokens":0},"answers":{"q":{"type":"choice","choice":"yes","probabilities":{"yes":0.9,"no":0.1},"confidence":0.8}}})).unwrap();
        assert!(request().validate(&r).is_ok());
        r.model = "other".into();
        assert!(request().validate(&r).is_err());
        r.model = MODEL.into();
        r.answers.clear();
        assert!(request().validate(&r).is_err());
    }
    #[tokio::test]
    async fn off_and_unapproved_assist_do_not_make_requests() {
        for mode in [Mode::Off, Mode::Assist] {
            let c = Client::new(Config {
                mode,
                api_key: "secret".into(),
                cloud_allowed: true,
                endpoint: "http://127.0.0.1:1".into(),
                ..Default::default()
            })
            .unwrap();
            assert!(matches!(
                c.evaluate("tenant", &request()).await,
                Err(Error::Disabled)
            ));
        }
    }
    #[tokio::test]
    async fn budget_is_scoped_and_reserved_before_network() {
        let c = Client::new(Config {
            mode: Mode::Shadow,
            api_key: "secret".into(),
            cloud_allowed: true,
            hourly_token_budget: 1,
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(
            c.evaluate("tenant", &request()).await,
            Err(Error::Budget)
        ));
    }
    #[test]
    fn scores_require_complete_distributions_and_correct_expectation() {
        let request = Request::new(
            serde_json::json!("state"),
            BTreeMap::from([(
                "score".into(),
                Question::Score {
                    instructions: serde_json::json!("quality"),
                    criteria: vec![serde_json::json!("low"), serde_json::json!("high")],
                },
            )]),
        );
        let mut response = Response {
            model: MODEL.into(),
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
            answers: BTreeMap::from([(
                "score".into(),
                Answer::Score {
                    score: 0.75,
                    legend: BTreeMap::from([
                        ("0".into(), "low".into()),
                        ("1".into(), "high".into()),
                    ]),
                    probabilities: BTreeMap::from([("0".into(), 0.25), ("1".into(), 0.75)]),
                    confidence: 0.6,
                },
            )]),
        };
        assert!(request.validate(&response).is_ok());
        if let Some(Answer::Score { score, .. }) = response.answers.get_mut("score") {
            *score = 1.5;
        }
        assert!(request.validate(&response).is_err());
    }
    #[tokio::test]
    async fn hosted_opt_out_and_oversize_are_rejected_before_io() {
        let client = Client::new(Config {
            mode: Mode::Shadow,
            api_key: "test".into(),
            endpoint: "http://127.0.0.1:1".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(
            client.evaluate("profile", &request()).await,
            Err(Error::Disabled)
        ));
        let mut large = request();
        large.state = serde_json::json!("x".repeat(MAX_REQUEST_BYTES));
        assert!(matches!(large.encode(), Err(Error::TooLarge)));
    }
    #[tokio::test]
    async fn rate_limit_and_invalid_json_fall_back_without_retry() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body) in [(429, "{}"), (200, "not-json")] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let handle = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                let received = socket.read(&mut bytes).await.unwrap();
                assert!(received > 0);
                socket.write_all(format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), listener.accept())
                        .await
                        .is_err()
                );
            });
            let client = Client::new(Config {
                mode: Mode::Shadow,
                cloud_allowed: true,
                api_key: "test".into(),
                endpoint,
                ..Default::default()
            })
            .unwrap();
            assert!(client.evaluate("tenant", &request()).await.is_err());
            handle.await.unwrap();
        }
    }
    #[tokio::test]
    async fn accepts_mock_http_and_checks_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let input = String::from_utf8_lossy(&buf[..n]);
            assert!(input.contains("POST /v1/systemone"));
            let body=json!({"model":MODEL,"answers":{"q":{"type":"choice","choice":"yes","probabilities":{"yes":0.9,"no":0.1},"confidence":0.8}},"usage":{"input_tokens":10,"output_tokens":0}}).to_string();
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let c = Client::new(Config {
            mode: Mode::Shadow,
            api_key: "test".into(),
            cloud_allowed: true,
            endpoint,
            ..Default::default()
        })
        .unwrap();
        assert!(c.evaluate("tenant", &request()).await.is_ok());
        handle.await.unwrap();
    }
}
