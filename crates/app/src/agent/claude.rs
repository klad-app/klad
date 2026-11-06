use super::{Delta, Provider, Reply, Request, Usage};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

pub const API: &str = "https://api.anthropic.com/v1";
pub const VERSION: &str = "2026-10-21";

pub struct Claude {
    pub key: String,
}

fn updates(model: &str) -> bool {
    ["claude-fable-5", "claude-opus-5-5", "claude-sonnet-5-5"]
        .iter()
        .any(|m| model.starts_with(m))
}

fn fallbacks(model: &str) -> bool {
    ["claude-fable-5-1", "claude-opus-5", "claude-sonnet-5-5"]
        .iter()
        .any(|m| model.starts_with(m))
}

pub fn body(request: &Request) -> Value {
    let mut tools = request.tools.clone();
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = json!({"type": "ephemeral"});
    }
    let mut body = json!({
        "model": request.model,
        "max_tokens": 64000,
        "stream": true,
        "system": [{"type": "text", "text": request.system, "cache_control": {"type": "ephemeral"}}],
        "tools": tools,
        "tool_choice": {"type": "auto"},
        "messages": request.messages,
        "cache_control": {"type": "ephemeral"},
        "output_config": {"effort": "medium"},
    });
    if updates(&request.model) {
        body["thinking"] = json!({"type": "adaptive", "display": "updates"});
    }
    if fallbacks(&request.model) {
        body["fallbacks"] = json!("default");
    }
    body
}

pub fn betas(model: &str) -> String {
    let mut list = Vec::new();
    if updates(model) {
        list.push("thinking-display-updates-2026-08-18");
    }
    if fallbacks(model) {
        list.push("server-side-fallback-2026-07-01");
    }
    list.join(",")
}

fn config(key: &str, extra: &[String]) -> String {
    let quoted = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut lines = vec![
        format!("header = \"x-api-key: {}\"", quoted(key)),
        format!("header = \"anthropic-version: {VERSION}\""),
    ];
    lines.extend(extra.iter().cloned());
    lines.join("\n") + "\n"
}

fn curl(key: &str, url: &str, extra: &[String], body: Option<&std::path::Path>) -> Result<std::process::Child> {
    let mut command = Command::new("curl");
    command.args(["-sS", "-N", "--no-buffer", "--fail-with-body", "--config", "-", url]);
    if let Some(path) = body {
        command.args(["-X", "POST", "-H", "content-type: application/json", "--data-binary"]);
        command.arg(format!("@{}", path.display()));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting curl, which klad uses to reach the API")?;
    child
        .stdin
        .take()
        .context("curl's input")?
        .write_all(config(key, extra).as_bytes())?;
    Ok(child)
}

fn api_error(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| text.trim().chars().take(400).collect())
}

pub fn models(key: &str) -> Result<Vec<String>> {
    let out = curl(key, &format!("{API}/models?limit=100"), &[], None)?.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        bail!("{}", api_error(&text));
    }
    let v: Value = serde_json::from_str(&text)?;
    Ok(v["data"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect())
        .unwrap_or_default())
}

#[derive(Default)]
pub struct Stream {
    blocks: Vec<Value>,
    partial: Vec<String>,
    pub stop: String,
    pub usage: Usage,
    pub error: Option<String>,
}

impl Stream {
    pub fn line(&mut self, data: &str, delta: &mut dyn FnMut(Delta)) {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let usage = |u: &Value, into: &mut Usage| {
            let n = |k: &str| u[k].as_u64();
            if let Some(v) = n("input_tokens") {
                into.input = v;
            }
            if let Some(v) = n("output_tokens") {
                into.output = v;
            }
            if let Some(v) = n("cache_read_input_tokens") {
                into.cache_read = v;
            }
            if let Some(v) = n("cache_creation_input_tokens") {
                into.cache_write = v;
            }
        };
        match event["type"].as_str().unwrap_or_default() {
            "message_start" => usage(&event["message"]["usage"], &mut self.usage),
            "content_block_start" => {
                let i = event["index"].as_u64().unwrap_or(0) as usize;
                while self.blocks.len() <= i {
                    self.blocks.push(Value::Null);
                    self.partial.push(String::new());
                }
                self.blocks[i] = event["content_block"].clone();
            }
            "content_block_delta" => {
                let i = event["index"].as_u64().unwrap_or(0) as usize;
                let (Some(block), d) = (self.blocks.get_mut(i), &event["delta"]) else {
                    return;
                };
                let append = |b: &mut Value, k: &str, t: &str| {
                    let now = b[k].as_str().unwrap_or_default().to_string();
                    b[k] = json!(now + t);
                };
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or_default();
                        append(block, "text", t);
                        delta(Delta::Text(t.to_string()));
                    }
                    "thinking_delta" => {
                        let t = d["thinking"].as_str().unwrap_or_default();
                        append(block, "thinking", t);
                        delta(Delta::Thinking(t.to_string()));
                    }
                    "signature_delta" => append(block, "signature", d["signature"].as_str().unwrap_or_default()),
                    "input_json_delta" => self.partial[i].push_str(d["partial_json"].as_str().unwrap_or_default()),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = event["index"].as_u64().unwrap_or(0) as usize;
                if let Some(block) = self.blocks.get_mut(i)
                    && block["type"] == "tool_use"
                {
                    let text = self.partial[i].trim();
                    block["input"] = match text.is_empty() {
                        true => json!({}),
                        false => serde_json::from_str::<Value>(text)
                            .ok()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({"__invalid": true})),
                    };
                }
            }
            "message_delta" => {
                if let Some(s) = event["delta"]["stop_reason"].as_str() {
                    self.stop = s.to_string();
                }
                usage(&event["usage"], &mut self.usage);
            }
            "error" => {
                self.error = Some(
                    event["error"]["message"]
                        .as_str()
                        .unwrap_or("the API stopped the reply")
                        .into(),
                )
            }
            _ => {}
        }
    }

    pub fn content(&self) -> Vec<Value> {
        self.blocks
            .iter()
            .filter(|b| !b.is_null())
            .map(|b| {
                let mut b = b.clone();
                if b["type"] == "tool_use" && b["input"].get("__invalid").is_some() {
                    b["input"] = json!({});
                }
                b
            })
            .collect()
    }

    pub fn calls(&self) -> Vec<Value> {
        self.blocks
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .cloned()
            .collect()
    }
}

impl Provider for Claude {
    fn send(&mut self, request: &Request, delta: &mut dyn FnMut(Delta)) -> Result<Reply> {
        let file = std::env::temp_dir().join(format!("klad-agent-{}-{}.json", std::process::id(), rand_tag()));
        std::fs::write(&file, serde_json::to_vec(&body(request))?)?;
        let beta = betas(&request.model);
        let extra: Vec<String> = match beta.is_empty() {
            true => Vec::new(),
            false => vec![format!("header = \"anthropic-beta: {beta}\"")],
        };
        let mut child = curl(&self.key, &format!("{API}/messages"), &extra, Some(&file))?;
        let mut stream = Stream::default();
        let mut raw = String::new();
        let stdout = child.stdout.take().context("curl's output")?;
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .for_each(|line| match line.strip_prefix("data:") {
                Some(data) => stream.line(data.trim(), delta),
                None if !line.starts_with("event:") => {
                    raw.push_str(&line);
                    raw.push('\n');
                }
                None => {}
            });
        let status = child.wait()?;
        let _ = std::fs::remove_file(&file);
        if let Some(e) = stream.error {
            return Err(anyhow!("the API: {e}"));
        }
        if !status.success() && stream.stop.is_empty() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut e, &mut err);
            }
            bail!(
                "the API: {}",
                api_error(if raw.trim().is_empty() { &err } else { &raw })
            );
        }
        let invalid = stream
            .calls()
            .iter()
            .filter(|c| c["input"].get("__invalid").is_some())
            .filter_map(|c| c["id"].as_str().map(str::to_string))
            .collect();
        let content = stream.content();
        if stream.stop == "refusal" {
            delta(Delta::Text("(the model declined this request)".into()));
        }
        Ok(Reply {
            content,
            stop: stream.stop,
            usage: stream.usage,
            invalid,
        })
    }
}

fn rand_tag() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

pub fn refused(key: &str) -> Option<&'static str> {
    match key {
        "" => Some("paste a key first"),
        k if k.starts_with("sk-ant-oat") => Some(
            "this is a Claude subscription token (from claude setup-token). Anthropic allows a Pro or Max subscription only in Claude Code and on claude.ai, so klad needs an API key from the Claude Console (console.anthropic.com), which starts with sk-ant-api",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subscription_token_is_refused_with_the_reason() {
        assert!(refused("sk-ant-oat01-abc").unwrap().contains("claude setup-token"));
        assert_eq!(refused(""), Some("paste a key first"));
        assert_eq!(refused("sk-ant-api03-abc"), None);
    }

    #[test]
    fn a_streamed_reply_with_thinking_text_and_a_tool_call_comes_back_whole() {
        let lines = [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":1200,"cache_read_input_tokens":900,"cache_creation_input_tokens":0}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Looking first."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Let me "}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"look."}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t1","name":"look","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"sca"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"le\": 0.5}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#,
        ];
        let mut stream = Stream::default();
        let mut said = Vec::new();
        lines.iter().for_each(|l| stream.line(l, &mut |d| said.push(d)));
        let content = stream.content();
        assert_eq!(
            content[0],
            json!({"type": "thinking", "thinking": "Looking first.", "signature": "sig"})
        );
        assert_eq!(content[1]["text"], "Let me look.");
        assert_eq!(content[2]["input"], json!({"scale": 0.5}));
        assert_eq!(stream.stop, "tool_use");
        assert_eq!(
            stream.usage,
            Usage {
                input: 1200,
                output: 42,
                cache_read: 900,
                cache_write: 0
            }
        );
        assert_eq!(said[1], Delta::Text("Let me ".into()));
        let request = Request {
            model: "claude-opus-5-5".into(),
            system: "draw".into(),
            tools: super::super::tools(true),
            messages: vec![json!({"role": "user", "content": "hi"})],
        };
        let b = body(&request);
        assert_eq!(
            b["tools"].as_array().unwrap().last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["thinking"]["display"], "updates");
        assert_eq!(b["fallbacks"], "default");
        assert!(betas("claude-opus-5-5").contains("server-side-fallback-2026-07-01"));
        assert!(
            !config("sk-ant-x", &[]).is_empty() && !format!("{:?}", b).contains("sk-ant"),
            "the key is never in the body"
        );
        let mut broken = Stream::default();
        [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t9","name":"apply","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"ops\": \"cut"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
        ]
        .iter()
        .for_each(|l| broken.line(l, &mut |_| {}));
        assert!(
            broken.calls()[0]["input"].get("__invalid").is_some(),
            "a cut-off input is caught, not run"
        );
    }
}
