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
        if items.is_empty() || value["total"].as_u64()
            .map_or(items.len() < 100, |total| keys.len() as u64 >= total)
        {
            return Ok(keys);
        }
    }
    Err("密钥数量超过同步上限，请先整理密钥".into())
}
fn group_id(key: &Value) -> Option<i64> {
    key["group_id"].as_i64().or_else(|| key["group"]["id"].as_i64())
}

fn fill_group(key: &mut Value, groups: &Value) {
    let Some(id) = group_id(key) else { return };
    if let Some(group) = groups.as_array().into_iter().flatten().find(|group| group["id"] == id) {
        key["group"] = group.clone();
    }
}

async fn hydrate_groups(keys: &mut [Value]) -> Result<(), String> {
    if keys.iter().any(|key| group_id(key).is_some() && key["group"]["platform"].as_str().unwrap_or("").is_empty()) {
        let groups = portal("/groups/available").await?;
        for key in keys.iter_mut().filter(|key| key["group"]["platform"].as_str().unwrap_or("").is_empty()) {
            fill_group(key, &groups);
        }
    }
    Ok(())
}

fn protocol(key: &Value) -> &'static str {
    match key["group"]["platform"].as_str().unwrap_or("") {
        "anthropic" | "minimax" => "anthropic-messages",
        "openai" | "grok" => "openai-responses",
        "deepseek" | "kimi" | "zhipu" | "opencode_go" | "typesafe" => {
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
    let mut key = portal(&format!("/keys/{id}")).await?;
    hydrate_groups(std::slice::from_mut(&mut key)).await?;
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
    let mut keys = all_keys().await?;
    hydrate_groups(&mut keys).await?;
    let keys = keys
        .into_iter()
        .map(|key| {
            json!({"id":key["id"],"name":key["name"],"group":key["group"]["name"],
            "protocol":protocol(&key),"usable":usable(&key),"status":key["status"],
            "codex_cli_only":key["group"]["codex_cli_only"] == true,
            "claude_code_only":key["group"]["claude_code_only"] == true,
            "quota":key["quota"],"quota_used":key["quota_used"],"expires_at":key["expires_at"]})
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"user":{"id":user["id"],"username":user["username"],"email":user["email"],"balance":user["balance"]},"keys":keys}),
    )
}
async fn models_for(key: &Value) -> Result<Vec<String>, String> {
    if let Some(id) = group_id(key) {
        let channels = portal("/channels/available").await?;
        if let Some(models) = channel_models(&channels, id) {
            return checked_models(models);
        }
    }
    if matches!(key["group"]["platform"].as_str(), Some("deepseek" | "anthropic")) {
        if let Ok(response) = client()?.get("https://nftai.fun/pricing-data.json")
            .header(reqwest::header::CACHE_CONTROL, "no-cache").send().await {
            if let Ok(catalog) = decode(response).await {
                if let Some(models) = public_group_models(&catalog, &key["group"]) {
                    return checked_models(models);
                }
            }
        }
    }
    let result = decode(
        client()?
            .get(format!("{ORIGIN}/v1/models"))
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
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
    let ids = array
        .iter()
        .filter_map(|v| v["id"].as_str())
        .filter(|id| match key["group"]["platform"].as_str() {
            Some("deepseek") => id.to_ascii_lowercase().starts_with("deepseek"),
            Some("anthropic") => id.to_ascii_lowercase().starts_with("claude"),
            _ => true,
        })
        .map(String::from)
        .collect::<Vec<_>>();
    checked_models(ids)
}

fn channel_models(channels: &Value, id: i64) -> Option<Vec<String>> {
    let mut result = None;
    for section in channels.as_array()?.iter().flat_map(|channel| channel["platforms"].as_array()).flatten() {
        if section["groups"].as_array().into_iter().flatten().any(|group| group["id"] == id) {
            result.get_or_insert_with(Vec::new).extend(section["supported_models"].as_array().into_iter().flatten()
                .filter_map(|model| model["name"].as_str()).map(String::from));
        }
    }
    result
}

fn public_group_models(catalog: &Value, group: &Value) -> Option<Vec<String>> {
    let name = group["name"].as_str()?;
    let platform = group["platform"].as_str()?;
    let entry = catalog["vendors"].as_array()?.iter().flat_map(|vendor| vendor["groups"].as_array()).flatten()
        .find(|entry| entry["name"] == name && entry["platform"] == platform)?;
    Some(entry["models"].as_array()?.iter().filter_map(|model| model["id"].as_str()).map(String::from).collect())
}

fn checked_models(mut ids: Vec<String>) -> Result<Vec<String>, String> {
    ids.retain(|id| !id.is_empty() && id.trim() == id && !id.chars().any(char::is_control) && !id.contains('*'));
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err("该密钥没有可用模型，请在网站检查分组".into());
    }
    Ok(ids)
}
#[tauri::command]
pub async fn nftai_models(key_id: i64, tool: String) -> Result<Vec<String>, String> {
    models_for_tool(&selected_key(key_id).await?, &tool).await
}

async fn models_for_tool(key: &Value, tool: &str) -> Result<Vec<String>, String> {
    if !compatible(tool, protocol(key))
        || (key["group"]["codex_cli_only"] == true && tool != "codex")
        || (key["group"]["claude_code_only"] == true && tool != "claude")
    {
        return Err("该密钥分组的协议或客户端限制不适用于此工具，请选择其他密钥".into());
    }
    models_for(key).await
}

fn compatible(app: &str, api: &str) -> bool {
    matches!(app, "claude" | "codex" | "dsh" | "opencode")
        && matches!(api, "anthropic-messages" | "openai-responses" | "openai-completions")
}

fn upstream_format(api: &str) -> &'static str {
    match api {
        "anthropic-messages" => "anthropic",
        "openai-completions" => "openai_chat",
        _ => "openai_responses",
    }
}
fn provider_config(
    app: &str,
    api: &str,
    secret: &str,
    model: &str,
    available_models: &[String],
) -> Result<Value, String> {
    if !compatible(app, api) {
        return Err("该密钥分组的协议不适用于此工具，请选择其他密钥".into());
    }
    let base = format!("{ORIGIN}/v1");
    if !available_models.iter().any(|id| id == model) {
        return Err("模型不属于该密钥，请重新获取模型列表".into());
    }
    Ok(match app {
        "claude" => json!({"env":{"ANTHROPIC_BASE_URL":if api == "anthropic-messages" { ORIGIN } else { &base },"ANTHROPIC_AUTH_TOKEN":secret,
            "ANTHROPIC_MODEL":model,"ANTHROPIC_DEFAULT_OPUS_MODEL":model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL":model,"ANTHROPIC_DEFAULT_HAIKU_MODEL":model}}),
        "codex" => with_codex_catalog(json!({"auth":{"OPENAI_API_KEY":secret}, "apiFormat":upstream_format(api), "config":format!(
            "model_provider = \"custom\"\nmodel = {}\n\n[model_providers.custom]\nname = \"NFTAI\"\nbase_url = {}\nwire_api = \"responses\"\nrequires_openai_auth = true\n",json!(model),json!(base))}), available_models),
        "dsh" => json!({"name":"NFTAI","kind":"custom","enabled":true,"api":api,
            "options":{"baseURL":base,"apiKey":secret},"models":named_models(available_models)}),
        "opencode" => {
            json!({"name":"NFTAI","npm":match api {"anthropic-messages"=>"@ai-sdk/anthropic", "openai-responses"=>"@ai-sdk/openai",_=>"@ai-sdk/openai-compatible"},
            "options":{"baseURL":base,"apiKey":secret},"models":named_models(available_models)})
        }
        _ => unreachable!(),
    })
}
fn named_models(models: &[String]) -> serde_json::Map<String, Value> {
    models
        .iter()
        .map(|model| (model.clone(), json!({"name": model})))
        .collect()
}

fn with_codex_catalog(mut settings: Value, models: &[String]) -> Value {
    let entries = models
        .iter()
        .map(|model| json!({"model": model}))
        .collect::<Vec<_>>();
    settings["modelCatalog"] = json!({"models": entries});
    settings
}
fn configure_desktop_provider(
    provider: &mut crate::Provider,
    api: &str,
    model: &str,
    available_models: &[String],
) -> bool {
    let proxy = api != "anthropic-messages"
        || available_models
            .iter()
            .any(|id| !crate::claude_desktop_config::is_claude_safe_model_id(id));
    let mut meta = crate::provider::ProviderMeta::default();
    if proxy {
        meta.claude_desktop_mode = Some(crate::provider::ClaudeDesktopMode::Proxy);
        meta.api_format = Some(upstream_format(api).to_string());
    }
    for id in std::iter::once(model)
        .chain(available_models.iter().map(String::as_str).filter(|id| *id != model))
        .take(crate::claude_desktop_config::MAX_INFERENCE_MODELS)
    {
        meta.claude_desktop_model_routes.insert(id.to_string(), crate::provider::ClaudeDesktopModelRoute {
            model: id.to_string(), label_override: None, supports_1m: None,
        });
    }
    provider.meta = Some(meta);
    proxy
}
#[tauri::command]
pub async fn nftai_apply(
    app: tauri::AppHandle,
    tool: String,
    key_id: i64,
    model: String,
) -> Result<Value, String> {
    let key = selected_key(key_id).await?;
    let models = models_for_tool(&key, &tool).await?;
    let api = protocol(&key);
    let settings = provider_config(
        &tool,
        api,
        key["key"].as_str().ok_or("密钥缺失")?,
        &model,
        &models,
    )?;
    let mut id = format!(
        "nftai-{}-{key_id}",
        key["user_id"].as_i64().ok_or("密钥缺少账户归属")?
    );
    let mut provider = crate::Provider::with_id(
        id.clone(),
        format!("NFTAI · {}", key["name"].as_str().unwrap_or("专用密钥")),
        settings,
        Some("https://nftai.fun".into()),
    );
    let routed = match tool.as_str() {
        "claude" => configure_desktop_provider(&mut provider, api, &model, &models),
        "codex" => api != "openai-responses",
        _ => false,
    };
    if routed {
        id.push_str("-routed");
        provider.id = id.clone();
    }
    let tool = if tool == "claude" { "claude-desktop".to_string() } else { tool };
    let existing = crate::commands::get_providers(app.state(), tool.clone()).await?;
    if existing.contains_key(&id) {
        crate::commands::update_provider(app.clone(), tool.clone(), provider, None, None).await?;
    } else {
        crate::commands::add_provider(app.clone(), tool.clone(), provider, Some(true), None)
            .await?;
    }
    let needs_activation = match tool.as_str() {
        "dsh" => !crate::dsh_config::provider_key_exists(&id).map_err(|err| err.to_string())?,
        "opencode" => !crate::opencode_config::get_providers()
            .map_err(|err| err.to_string())?
            .contains_key(&id),
        _ => true,
    };
    let routed_app = match tool.as_str() {
        "codex" => Some(crate::AppType::Codex),
        "claude-desktop" => Some(crate::AppType::ClaudeDesktop),
        _ => None,
    };
    let proxy_app = routed_app.filter(|app| routed || crate::mode::current::is_proxy(app));
    let warnings = if let Some(target) = proxy_app.as_ref() {
        crate::mode::controller::enter_with_route(app.state::<crate::AppState>().inner(), target, false, Some(&id)).await?;
        Vec::new()
    } else if needs_activation {
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
    Ok(json!({"providerId":id,"warnings":warnings,"message":if tool=="dsh" {
        format!("已同步 {} 个模型。请在 DSH 模型选择器中选择 NFTAI；如果列表未刷新，请重启 DSH。", models.len())
    } else {
        let count = if tool == "claude-desktop" {
            models.len().min(crate::claude_desktop_config::MAX_INFERENCE_MODELS)
        } else {
            models.len()
        };
        let routing = if proxy_app.is_some() { "使用此配置时请保持工具箱运行。" } else { "" };
        format!("已同步 {count} 个模型。请新开会话；如果列表未刷新，请完全退出并重启工具。{routing}")
    }}))
}

#[tauri::command]
pub async fn nftai_launch(tool: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::nftai_tools::launch(&tool))
        .await.map_err(|e| format!("启动失败：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_catalogs_do_not_mix_keys_or_platforms() {
        let channels = json!([{"platforms":[
            {"groups":[{"id":2}],"supported_models":[{"name":"claude-fable-5"}]},
            {"groups":[{"id":19}],"supported_models":[{"name":"deepseek-future"}]}
        ]}]);
        assert_eq!(channel_models(&channels, 19).unwrap(), ["deepseek-future"]);
        assert_eq!(channel_models(&channels, 2).unwrap(), ["claude-fable-5"]);
        assert_eq!(channel_models(&channels, 42), None);
        let mut key = json!({"group_id":2});
        fill_group(&mut key, &json!([{"id":2,"name":"Claude Plus","platform":"anthropic"}]));
        assert_eq!(protocol(&key), "anthropic-messages");
        let catalog = json!({"vendors":[{"groups":[
            {"name":"DS","platform":"deepseek","models":[{"id":"deepseek-chat"}]},
            {"name":"Claude Plus","platform":"anthropic","models":[{"id":"claude-next"}]}
        ]}]});
        assert_eq!(public_group_models(&catalog, &key["group"]).unwrap(), ["claude-next"]);
    }
    #[test]
    fn codex_catalog_contains_all_models_for_the_selected_key() {
        let models = ["gpt-6.2-astra", "gpt-6.2-sol", "gpt-6.1-sol"].map(String::from);
        let settings =
            provider_config("codex", "openai-responses", "test-secret", &models[2], &models).unwrap();
        let config = settings["config"].as_str().unwrap();
        let plan = crate::codex_config::plan_codex_model_catalog(
            &settings,
            config,
            crate::codex_config::CodexCatalogToolProfile::NativeResponses,
        )
        .unwrap();
        let catalog = plan.catalog.unwrap();
        let slugs = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["slug"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(slugs, ["gpt-6.2-astra", "gpt-6.2-sol", "gpt-6.1-sol"]);
        let parsed: toml::Value = config.parse().unwrap();
        assert_eq!(parsed["model"].as_str(), Some("gpt-6.1-sol"));
    }
    #[test]
    fn dsh_and_opencode_import_all_key_models() {
        let models = ["existing-model", "future-model"].map(String::from);
        for tool in ["dsh", "opencode"] {
            let config = provider_config(tool, "openai-responses", "test-secret", &models[0], &models).unwrap();
            assert_eq!(config["models"], json!({
                "existing-model": {"name": "existing-model"},
                "future-model": {"name": "future-model"}
            }));
            assert!(provider_config(tool, "openai-responses", "test-secret", "removed-model", &models).is_err());
        }
    }
    #[test]
    fn desktop_profile_contains_selected_gateway_and_model() {
        let models = ["claude-sonnet-4-5", "claude-opus-4-8"].map(String::from);
        let settings = provider_config("claude", "anthropic-messages", "test-secret", &models[0], &models).unwrap();
        let mut provider = crate::Provider::with_id("test".into(), "test".into(), settings, None);
        assert!(!configure_desktop_provider(&mut provider, "anthropic-messages", &models[0], &models));
        let profile = crate::claude_desktop_config::direct_target(Some(&provider)).unwrap().unwrap();
        assert_eq!(profile["inferenceProvider"], "gateway");
        assert_eq!(profile["inferenceGatewayBaseUrl"], ORIGIN);
        assert_eq!(profile["inferenceGatewayApiKey"], "test-secret");
        let menu = profile["inferenceModels"].as_array().unwrap();
        assert_eq!(menu.len(), 2);
        assert!(menu.contains(&json!("claude-sonnet-4-5")));
        assert!(menu.contains(&json!("claude-opus-4-8")));
    }
    #[test]
    fn desktop_routes_non_claude_model_alias_to_original_model() {
        let models = ["deepseek-chat", "kimi-k3"].map(String::from);
        let api = "openai-completions";
        let settings = provider_config("claude", api, "test-secret", &models[0], &models).unwrap();
        let mut provider = crate::Provider::with_id("test".into(), "test".into(), settings, None);
        assert!(configure_desktop_provider(&mut provider, api, &models[0], &models));
        crate::claude_desktop_config::validate_proxy_provider(&provider).unwrap();
        let routes = crate::claude_desktop_config::proxy_model_routes(&provider).unwrap();
        assert_eq!(routes.len(), 2);
        for route in routes {
            let request = crate::claude_desktop_config::map_proxy_request_model(
                json!({"model":route.route_id}),
                &provider,
            ).unwrap();
            assert_eq!(request["model"], route.upstream_model);
        }
    }
    #[test]
    fn codex_uses_responses_to_client_and_bridges_upstream_protocol() {
        use crate::proxy::providers::{
            should_convert_codex_responses_to_anthropic, should_convert_codex_responses_to_chat,
        };
        let models = ["non-gpt-model"].map(String::from);
        for api in ["openai-completions", "anthropic-messages", "openai-responses"] {
            let settings = provider_config("codex", api, "test-secret", &models[0], &models).unwrap();
            let config: toml::Value = settings["config"].as_str().unwrap().parse().unwrap();
            assert_eq!(config["model_providers"]["custom"]["wire_api"].as_str(), Some("responses"));
            assert_eq!(config["model"].as_str(), Some("non-gpt-model"));
            let provider = crate::Provider::with_id("test".into(), "test".into(), settings, None);
            assert_eq!(should_convert_codex_responses_to_chat(&provider, "/v1/responses"), api == "openai-completions");
            assert_eq!(should_convert_codex_responses_to_anthropic(&provider, "/v1/responses"), api == "anthropic-messages");
        }
    }
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
        for tool in ["codex", "claude", "opencode", "dsh"] {
            assert!(!compatible(tool, "unknown"));
            assert!(compatible(tool, "openai-completions"));
            assert!(compatible(tool, "openai-responses"));
            assert!(compatible(tool, "anthropic-messages"));
        }
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
        let model = "model\"\nmalicious=true";
        let config = provider_config(
            "codex",
            "openai-responses",
            "secret",
            model,
            &[model.to_string()],
        )
        .unwrap();
        let parsed: toml::Value = config["config"].as_str().unwrap().parse().unwrap();
        assert_eq!(parsed["model"].as_str().unwrap(), model);
        assert!(parsed.get("malicious").is_none());
        assert!(provider_config("claude", "unknown", "secret", "ds", &["ds".into()]).is_err());
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
