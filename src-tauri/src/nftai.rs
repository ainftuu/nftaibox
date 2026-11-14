//! NFTAI portal integration. Account credentials stay in the native credential vault;
//! the renderer receives only public account data and masked key descriptors.
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::post,
    Form, Router,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, Notify};

const ORIGIN: &str = "https://api.nftai.fun";
const VAULT: &str = "fun.nftai.toolbox";
static LOGIN_LOCK: Lazy<Arc<Mutex<()>>> = Lazy::new(|| Arc::new(Mutex::new(())));
static REQUEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static LOGIN_ERROR: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
static LOGIN_CANCEL: Lazy<Mutex<Option<Arc<Notify>>>> = Lazy::new(|| Mutex::new(None));
static LOGIN_STATUS: Lazy<Mutex<&'static str>> = Lazy::new(|| Mutex::new("idle"));

fn supports_toolbox(value: &Value) -> bool {
    value["protocol"] == 1
        && value["authorize_path"] == "/toolbox/authorize"
        && value["session_path"] == "/api/v1/auth/toolbox-session"
}

async fn check_authorization_deployment() -> Result<(), String> {
    let response = client()?.get(format!("{ORIGIN}/api/v1/auth/toolbox-capabilities"))
        .send().await.map_err(|_| "无法检查 NFTAI 授权服务，请检查网络后重试")?;
    let missing = "网站尚未部署工具箱授权服务，请联系站长更新授权补丁。浏览器登录成功不等于工具箱已获授权。";
    if !response.status().is_success() { return Err(missing.into()); }
    let value = decode(response).await.map_err(|_| missing.to_string())?;
    if !supports_toolbox(&value) { return Err(missing.into()); }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
}
fn vault() -> Result<keyring::Entry, String> {
    keyring::Entry::new(VAULT, "portal-session").map_err(|_| "无法访问系统凭据库".into())
}
fn load_session() -> Result<Session, String> {
    let text = vault()?
        .get_password()
        .map_err(|_| "请先登录 NFTAI".to_string())?;
    serde_json::from_str(&text).map_err(|_| "登录状态失效，请重新登录".into())
}
fn save_session(value: &Session) -> Result<(), String> {
    vault()?
        .set_password(&serde_json::to_string(value).map_err(|_| "无法保存登录状态")?)
        .map_err(|_| "系统凭据库拒绝保存登录状态，请解锁后重试".into())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化网络连接".into())
}
async fn decode(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "NFTAI 接口返回 HTTP {}，请检查登录或稍后重试",
            status.as_u16()
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "NFTAI 接口返回格式异常")?;
    if value
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|n| n != 0)
    {
        return Err("NFTAI 接口拒绝请求，请重新登录或检查账户状态".into());
    }
    Ok(value.get("data").cloned().unwrap_or(value))
}
async fn portal(path: &str) -> Result<Value, String> {
    let _guard = REQUEST_LOCK.lock().await;
    let mut session = load_session()?;
    let http = client()?;
    let mut response = http
        .get(format!("{ORIGIN}/api/v1{path}"))
        .bearer_auth(&session.access_token)
        .send()
        .await
        .map_err(|_| "连接 NFTAI 失败，请检查网络")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let refreshed = decode(
            http.post(format!("{ORIGIN}/api/v1/auth/refresh"))
                .json(&json!({"refresh_token":session.refresh_token}))
                .send()
                .await
                .map_err(|_| "刷新登录失败，请重新登录")?,
        )
        .await?;
        session.access_token = refreshed["access_token"]
            .as_str()
            .ok_or("登录状态失效")?
            .into();
        if let Some(token) = refreshed["refresh_token"].as_str() {
            session.refresh_token = token.into();
        }
        save_session(&session)?;
        response = http
            .get(format!("{ORIGIN}/api/v1{path}"))
            .bearer_auth(&session.access_token)
            .send()
            .await
            .map_err(|_| "连接 NFTAI 失败")?;
    }
    decode(response).await
}

#[derive(Deserialize)]
struct CallbackForm {
    state: String,
    access_token: String,
    refresh_token: String,
}
struct CallbackState {
    nonce: String,
    app: tauri::AppHandle,
    done: Notify,
    consumed: AtomicBool,
}
fn valid_callback(headers: &HeaderMap, nonce: &str, form: &CallbackForm) -> bool {
    headers.get("origin").and_then(|v| v.to_str().ok()) == Some(ORIGIN)
        && nonce == form.state
        && !form.access_token.is_empty()
        && !form.refresh_token.is_empty()
        && form.access_token.len() < 16384
        && form.refresh_token.len() < 16384
}
fn login_success_response() -> Response {
    // Finish the form navigation before navigating home. Chromium checks 3xx
    // destinations against the submitting page's form-action policy, which
    // permits loopback but not nftai.fun. A committed HTML document has its own
    // navigation context. No credentials are included in the document or URL.
    (
        [
            ("cache-control", "no-store"),
            ("referrer-policy", "no-referrer"),
            ("content-security-policy", "default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"),
        ],
        Html("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><meta http-equiv=\"refresh\" content=\"0;url=https://nftai.fun/\"><title>登录成功 · NFTAI</title></head><body><p>工具箱登录成功，正在返回 NFTAI 首页…</p><a href=\"https://nftai.fun/\" rel=\"noreferrer\">前往 NFTAI 首页</a></body></html>"),
    ).into_response()
}
async fn callback(
    State(state): State<Arc<CallbackState>>,
    headers: HeaderMap,
    Form(form): Form<CallbackForm>,
) -> Response {
    if !valid_callback(&headers, &state.nonce, &form)
        || state
            .consumed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return (
            StatusCode::FORBIDDEN,
            Html("Authorization rejected. Please restart login in NFTAI Toolbox."),
        ).into_response();
    }
    // Verify the supplied token against our own server before storing it.
    let result = async {
        let http = client()?;
        decode(
            http.get(format!("{ORIGIN}/api/v1/auth/me"))
                .bearer_auth(&form.access_token)
                .send()
                .await
                .map_err(|_| "登录验证失败")?,
        )
        .await?;
        let _guard = REQUEST_LOCK.lock().await;
        save_session(&Session {
            access_token: form.access_token,
            refresh_token: form.refresh_token,
        })
    }
    .await;
    match result {
        Ok(()) => {
            *LOGIN_STATUS.lock().await = "authorized";
            let _ = state.app.emit("nftai-login", true);
            state.done.notify_one();
            login_success_response()
        }
        Err(error) => {
            state.consumed.store(false, Ordering::SeqCst);
            *LOGIN_ERROR.lock().await = Some(error);
            (
                StatusCode::BAD_GATEWAY,
                Html("Login could not be saved. Return to NFTAI Toolbox and retry."),
            ).into_response()
        }
    }
}

#[tauri::command]
pub async fn nftai_login_start(app: tauri::AppHandle) -> Result<String, String> {
    let guard = LOGIN_LOCK
        .clone()
        .try_lock_owned()
        .map_err(|_| "浏览器登录正在进行，请完成登录或等待五分钟后重试")?;
    check_authorization_deployment().await?;
    *LOGIN_ERROR.lock().await = None;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "无法启动本地登录回调")?;
    let port = listener
        .local_addr()
        .map_err(|_| "无法获取回调端口")?
        .port();
    let nonce = uuid::Uuid::new_v4().to_string();
    let url = format!("{ORIGIN}/toolbox/authorize?port={port}&state={nonce}");
    let state = Arc::new(CallbackState {
        nonce,
        app,
        done: Notify::new(),
        consumed: AtomicBool::new(false),
    });
    let cancel = Arc::new(Notify::new());
    *LOGIN_CANCEL.lock().await = Some(cancel.clone());
    *LOGIN_STATUS.lock().await = "waiting";
    let router = Router::new()
        .route("/authorize", post(callback))
        .layer(axum::extract::DefaultBodyLimit::max(40000))
        .with_state(state.clone());
    // Own the lock within the server task. A second start cannot replace an active nonce.
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let served = axum::serve(listener, router).with_graceful_shutdown(async move {
            tokio::select! {
                _ = state.done.notified() => {},
                _=cancel.notified()=>{*LOGIN_STATUS.lock().await = "cancelled";},
                _ = tokio::time::sleep(Duration::from_secs(300)) => {
                    *LOGIN_STATUS.lock().await = "expired";
                    *LOGIN_ERROR.lock().await = Some("浏览器授权已超时，请从工具箱重新发起登录。".into());
                }
            }
        }).await;
        if served.is_err() {
            *LOGIN_STATUS.lock().await = "failed";
            *LOGIN_ERROR.lock().await = Some("本机授权回调服务已停止，请重新登录。".into());
        }
        *LOGIN_CANCEL.lock().await = None;
    });
    Ok(url)
}

#[tauri::command]
pub async fn nftai_login_cancel() {
    if let Some(cancel) = LOGIN_CANCEL.lock().await.take() {
        cancel.notify_one();
    }
}

#[tauri::command]
pub async fn nftai_login_error() -> Option<String> {
    LOGIN_ERROR.lock().await.take()
}

#[tauri::command]
pub async fn nftai_login_status() -> Value {
    json!({"status":*LOGIN_STATUS.lock().await,"error":LOGIN_ERROR.lock().await.clone()})
}

#[tauri::command]
pub async fn nftai_logout() -> Result<(), String> {
    let _guard = REQUEST_LOCK.lock().await;
    if let Ok(session) = load_session() {
        if let Ok(http) = client() {
            let _ = http
                .post(format!("{ORIGIN}/api/v1/auth/logout"))
                .json(&json!({"refresh_token":session.refresh_token}))
                .send()
                .await;
        }
    }
    match vault()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("无法清除系统凭据，请解锁凭据库后重试".into()),
    }
}

async fn all_keys() -> Result<Vec<Value>, String> {
    let mut keys = Vec::new();
    for page in 1..=100 {
        let value = portal(&format!("/keys?page={page}&page_size=100")).await?;
        let items = value["items"].as_array().ok_or("密钥列表格式异常")?;
        keys.extend(items.iter().cloned());
        if items.len() < 100
            || value["total"]
                .as_u64()
                .is_some_and(|n| keys.len() as u64 >= n)
        {
            return Ok(keys);
        }
    }
    Err("密钥数量超过同步上限，请先整理密钥".into())
}
fn protocol(key: &Value) -> &'static str {
    match key["group"]["platform"].as_str().unwrap_or("") {
        "anthropic" => "anthropic-messages",
        "openai" => "openai-responses",
        "deepseek" | "kimi" | "zhipu" | "minimax" | "grok" | "opencode_go" | "typesafe" => {
            "openai-completions"
        }
        _ => "unknown",
    }
}
fn usable(key: &Value) -> bool {
    key["status"] == "active"
        && key["group"]["status"] != "inactive"
        && (key["quota"].as_f64().unwrap_or(0.0) <= 0.0
            || key["quota_used"].as_f64().unwrap_or(0.0) < key["quota"].as_f64().unwrap_or(0.0))
        && key["expires_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|time| time.timestamp() > chrono::Utc::now().timestamp())
}
async fn selected_key(id: i64) -> Result<Value, String> {
    let key = portal(&format!("/keys/{id}")).await?;
    if !usable(&key) {
        return Err("该密钥已停用、到期或额度不足，请同步后重新选择".into());
    }
    let secret = key["key"].as_str().ok_or("服务端未提供完整密钥")?;
    if secret.contains('*') || secret.contains('…') || secret.contains("...") || secret.len() < 12
    {
        return Err("后台只返回脱敏密钥，无法配置，请在网站创建工具箱专用密钥".into());
    }
    Ok(key)
}
#[tauri::command]
pub async fn nftai_snapshot() -> Result<Value, String> {
    let user = portal("/auth/me").await?;
    let keys = all_keys()
        .await?
        .into_iter()
        .map(|key| {
            json!({"id":key["id"],"name":key["name"],"group":key["group"]["name"],
            "protocol":protocol(&key),"usable":usable(&key),"status":key["status"],
            "quota":key["quota"],"quota_used":key["quota_used"],"expires_at":key["expires_at"]})
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"user":{"id":user["id"],"username":user["username"],"email":user["email"],"balance":user["balance"]},"keys":keys}),
    )
}
async fn models_for(key: &Value) -> Result<Vec<String>, String> {
    let result = decode(
        client()?
            .get(format!("{ORIGIN}/v1/models"))
            .bearer_auth(key["key"].as_str().ok_or("密钥缺失")?)
            .send()
            .await
            .map_err(|_| "获取模型失败，请检查网络")?,
    )
    .await?;
    let array = result
        .as_array()
        .or_else(|| result["data"].as_array())
        .ok_or("模型列表格式异常")?;
    let mut ids = array
        .iter()
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err("该密钥没有可用模型，请在网站检查分组".into());
    }
    Ok(ids)
}
#[tauri::command]
pub async fn nftai_models(key_id: i64) -> Result<Vec<String>, String> {
    models_for(&selected_key(key_id).await?).await
}

fn compatible(app: &str, api: &str) -> bool {
    match app {
        "claude" => api == "anthropic-messages",
        "codex" => api == "openai-responses",
        "dsh" | "opencode" => matches!(
            api,
            "anthropic-messages" | "openai-responses" | "openai-completions"
        ),
        _ => false,
    }
}
fn provider_config(app: &str, api: &str, secret: &str, model: &str) -> Result<Value, String> {
    if !compatible(app, api) {
        return Err("该密钥分组的协议不适用于此工具，请选择其他密钥".into());
    }
    let base = format!("{ORIGIN}/v1");
    let models = json!({model:{"name":model}});
    Ok(match app {
        "claude" => json!({"env":{"ANTHROPIC_BASE_URL":ORIGIN,"ANTHROPIC_AUTH_TOKEN":secret,
            "ANTHROPIC_MODEL":model,"ANTHROPIC_DEFAULT_OPUS_MODEL":model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL":model,"ANTHROPIC_DEFAULT_HAIKU_MODEL":model}}),
        "codex" => json!({"auth":{"OPENAI_API_KEY":secret}, "config":format!(
            "model_provider = \"custom\"\nmodel = {}\n\n[model_providers.custom]\nname = \"NFTAI\"\nbase_url = {}\nwire_api = \"responses\"\nrequires_openai_auth = true\n",json!(model),json!(base))}),
        "dsh" => json!({"name":"NFTAI","kind":"custom","enabled":true,"api":api,
            "options":{"baseURL":base,"apiKey":secret},"models":models}),
        "opencode" => {
            json!({"name":"NFTAI","npm":match api {"anthropic-messages"=>"@ai-sdk/anthropic", "openai-responses"=>"@ai-sdk/openai",_=>"@ai-sdk/openai-compatible"},
            "options":{"baseURL":base,"apiKey":secret},"models":models})
        }
        _ => unreachable!(),
    })
}
#[tauri::command]
pub async fn nftai_apply(
    app: tauri::AppHandle,
    tool: String,
    key_id: i64,
    model: String,
) -> Result<Value, String> {
    let key = selected_key(key_id).await?;
    if !models_for(&key).await?.contains(&model) {
        return Err("模型不属于该密钥，请重新获取模型列表".into());
    }
    let settings = provider_config(
        &tool,
        protocol(&key),
        key["key"].as_str().ok_or("密钥缺失")?,
        &model,
    )?;
    let id = format!(
        "nftai-{}-{key_id}",
        key["user_id"].as_i64().ok_or("密钥缺少账户归属")?
    );
    let provider = crate::Provider::with_id(
        id.clone(),
        format!("NFTAI · {}", key["name"].as_str().unwrap_or("专用密钥")),
        settings,
        Some("https://nftai.fun".into()),
    );
    let existing = crate::commands::get_providers(app.state(), tool.clone()).await?;
    if existing.contains_key(&id) {
        crate::commands::update_provider(app.clone(), tool.clone(), provider, None, None).await?;
    } else {
        crate::commands::add_provider(app.clone(), tool.clone(), provider, Some(true), None)
            .await?;
    }
    let warnings = if matches!(tool.as_str(), "claude" | "codex") {
        crate::commands::switch_provider(app.clone(), tool.clone(), id.clone())
            .await?
            .warnings
    } else {
        Vec::new()
    };
    if tool == "opencode" {
        crate::opencode_config::set_default_model(&format!("{id}/{model}"))
            .map_err(|_| "已导入密钥，但设置默认模型失败，请在工具内选择模型")?;
    }
    Ok(
        json!({"providerId":id,"warnings":warnings,"message":if tool=="dsh" {"已导入。首次打开 DSH 时，在模型选择器中选择 NFTAI 下的模型。"} else {"已配置。请新开会话，或完全退出并重启工具后使用。"}}),
    )
}

#[tauri::command]
pub async fn nftai_launch(tool: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::nftai_tools::launch(&tool))
        .await.map_err(|e| format!("启动失败：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn successful_login_commits_html_before_navigating_home() {
        let response = login_success_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("location"));
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(body.contains("content=\"0;url=https://nftai.fun/\""));
        assert!(body.contains("href=\"https://nftai.fun/\""));
        assert!(!body.contains("access_token"));
        assert!(!body.contains("refresh_token"));
    }
    #[test]
    fn login_rejects_missing_or_incompatible_server_deployment() {
        assert!(supports_toolbox(&json!({"protocol":1,"authorize_path":"/toolbox/authorize","session_path":"/api/v1/auth/toolbox-session"})));
        assert!(!supports_toolbox(&json!({"protocol":1})));
        assert!(!supports_toolbox(&json!({"protocol":2,"authorize_path":"/toolbox/authorize","session_path":"/api/v1/auth/toolbox-session"})));
        assert!(!supports_toolbox(&json!({"protocol":1,"authorize_path":"https://other.example/","session_path":"/api/v1/auth/toolbox-session"})));
    }
    #[test]
    fn protocols_are_not_guessed_for_unknown_groups() {
        assert_eq!(
            protocol(&json!({"group":{"platform":"composite"}})),
            "unknown"
        );
        assert!(!compatible("codex", "openai-completions"));
        assert!(!compatible("claude", "openai-responses"));
    }
    #[test]
    fn callback_requires_our_origin_and_nonce() {
        let mut headers = HeaderMap::new();
        let form = CallbackForm {
            state: "nonce".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
        };
        assert!(!valid_callback(&headers, "nonce", &form));
        headers.insert("origin", ORIGIN.parse().unwrap());
        assert!(valid_callback(&headers, "nonce", &form));
        assert!(!valid_callback(&headers, "different", &form));
    }
    #[test]
    fn toml_model_is_escaped_and_other_models_are_untouched() {
        let config = provider_config(
            "codex",
            "openai-responses",
            "secret",
            "model\"\nmalicious=true",
        )
        .unwrap();
        let parsed: toml::Value = config["config"].as_str().unwrap().parse().unwrap();
        assert_eq!(parsed["model"].as_str().unwrap(), "model\"\nmalicious=true");
        assert!(parsed.get("malicious").is_none());
        assert!(provider_config("claude", "openai-completions", "secret", "ds").is_err());
    }
    #[test]
    fn expired_keys_are_unusable() {
        assert!(!usable(
            &json!({"status":"active","quota":10,"quota_used":10})
        ));
        assert!(!usable(
            &json!({"status":"active","expires_at":"2000-01-01T00:00:00Z"})
        ));
        assert!(!usable(&json!({"status":"inactive"})));
        assert!(usable(&json!({"status":"active","expires_at":null})));
    }
}
