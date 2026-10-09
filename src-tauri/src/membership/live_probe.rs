//! Explicit opt-in, synthetic live acceptance probe. Never runs in normal tests.
//! Credentials stay inside the existing native membership service.
use super::{protocol, service::MembershipService};
use crate::storage::Storage;
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc, time::Duration};

const MODEL: &str = "gpt-6-luna";
const LIMIT: usize = 128 * 1024 * 1024;

fn base() -> Value {
    json!({"model":MODEL,"store":false,"stream":true,"reasoning":{"effort":"low"},
        "input":[{"role":"user","content":"Reply with only LUNA-PROBE-OK. Do not use tools."}]})
}
fn safe_tag(value: &Value) -> Value {
    value
        .as_str()
        .filter(|s| {
            s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
        .map(|s| json!(s))
        .unwrap_or(Value::Null)
}
fn safe_request_error(status: u16, value: &Value) -> Value {
    if !matches!(status, 200 | 400 | 422) {
        return Value::Null;
    }
    let message = value["error"]["message"]
        .as_str()
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["error"].as_str());
    message
        .filter(|s| {
            s.len() <= 300
                && !s.chars().any(char::is_control)
                && ![
                    "Bearer",
                    "sk-",
                    "access_token",
                    "refresh_token",
                    "@",
                    "://",
                    "eyJ",
                ]
                .iter()
                .any(|part| s.contains(part))
        })
        .map(|s| json!(s))
        .unwrap_or(Value::Null)
}
fn stream_values(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}
fn terminal(values: &[Value]) -> Option<Value> {
    values
        .iter()
        .rev()
        .find_map(|event| match event["type"].as_str() {
            Some("response.completed" | "response.failed" | "response.incomplete") => {
                Some(event["response"].clone())
            }
            Some("error") => Some(json!({"status":"error","error":event["error"].clone()})),
            _ => None,
        })
}
struct Observation {
    report: Value,
    items: Vec<Value>,
}

async fn probe(
    service: &Arc<MembershipService>,
    account: &str,
    name: &str,
    payload: Value,
) -> Result<Observation, String> {
    let cancellation = service.session_cancellation(account)?;
    let (credentials, _) = service.credentials(account, &cancellation).await?;
    let operation = async {
        let request = service
            .client
            .post(format!("{}/responses", protocol::RESOURCE))
            .timeout(Duration::from_secs(600))
            .bearer_auth(&credentials.access_token)
            .json(&payload);
        let mut response = service.send(request, &cancellation).await?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        let mut scanned = 0;
        let mut values = Vec::new();
        let mut terminal_seen = false;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Probe response interrupted")?
        {
            if bytes.len() + chunk.len() > LIMIT {
                return Err("Probe response exceeded its bound".into());
            }
            bytes.extend_from_slice(&chunk);
            // Scan only complete new lines: reparsing the entire accumulated SSE
            // stream after every chunk becomes quadratic at full output size.
            while let Some(end) = bytes[scanned..].iter().position(|b| *b == b'\n') {
                let next = scanned + end + 1;
                for event in stream_values(&bytes[scanned..next]) {
                    terminal_seen |= matches!(
                        event["type"].as_str(),
                        Some(
                            "response.completed"
                                | "response.failed"
                                | "response.incomplete"
                                | "error"
                        )
                    );
                    // Deltas can dominate wire size; terminal output is authoritative.
                    if !event["type"].as_str().unwrap_or("").ends_with(".delta") {
                        values.push(event);
                    }
                }
                scanned = next;
            }
            if payload["stream"] == true && terminal_seen {
                break;
            }
        }
        values.extend(stream_values(&bytes[scanned..]));
        let response_value = terminal(&values)
            .or_else(|| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let mut items = response_value["output"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if items.is_empty() {
            items = values
                .iter()
                .filter(|v| v["type"] == "response.output_item.done")
                .map(|v| v["item"].clone())
                .collect();
        }
        let text: String = items
            .iter()
            .filter(|v| v["type"] == "message")
            .flat_map(|v| v["content"].as_array().into_iter().flatten())
            .filter(|v| v["type"] == "output_text")
            .filter_map(|v| v["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let types: BTreeSet<_> = values.iter().filter_map(|v| v["type"].as_str()).collect();
        let calls:Vec<_>=items.iter().filter(|v|matches!(v["type"].as_str(),Some("function_call"|"custom_tool_call"|"web_search_call")))
            .map(|v|json!({"type":safe_tag(&v["type"]),"name":safe_tag(&v["name"]),"namespace":safe_tag(&v["namespace"]),"status":safe_tag(&v["status"])})).collect();
        let error = &response_value["error"];
        let report = json!({"case":name,"http":status,"status":safe_tag(&response_value["status"]),
            "errorCode":safe_tag(&error["code"]),"errorParam":safe_tag(&error["param"]),"requestError":safe_request_error(status,&response_value),
            "incompleteReason":safe_tag(&response_value["incomplete_details"]["reason"]),
            "responseFields":{"reasoning":response_value["reasoning"],"max_output_tokens":response_value["max_output_tokens"],"temperature":response_value["temperature"],"top_p":response_value["top_p"]},
            "inputTokens":response_value["usage"]["input_tokens"],
            "wireBytes":bytes.len(),"outputCharacters":text.chars().count(),
            "outputTokens":response_value["usage"]["output_tokens"],"text":text.chars().take(400).collect::<String>(),"toolCalls":calls,"events":types});
        Ok(Observation { report, items })
    };
    tokio::select! {
        _=cancellation.cancelled()=>Err("Membership session cancelled".into()),
        result=tokio::time::timeout(Duration::from_secs(610),operation)=>result.map_err(|_|"Probe timed out")?,
    }
}

fn save(path: &std::path::Path, reports: &[Value]) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(
            &json!({"model":MODEL,"reasoning":if reports.first().is_some_and(|r|r["case"]=="reasoning_none") {"none"} else {"low"},"syntheticOnly":true,"results":reports}),
        )
        .map_err(|_| "Could not serialize probe receipt")?,
    )
    .map_err(|_| "Could not save probe receipt".into())
}
fn quota_stop(report: &Value) -> bool {
    matches!(report["http"].as_u64(), Some(401 | 402 | 429))
        || matches!(
            report["errorCode"].as_str(),
            Some(
                "subscription_sharing_usage_limit_exceeded"
                    | "subscription_sharing_usage_unavailable"
            )
        )
}

#[tokio::test]
#[ignore = "Explicit owner-authorized live membership probe; consumes plan allowance"]
async fn authorized_luna_probe() {
    assert_eq!(
        std::env::var("CROWCLAW_LUNA_PROBE_ALLOW").ok().as_deref(),
        Some("1"),
        "Live probe requires explicit opt-in"
    );
    let profile = std::path::PathBuf::from(
        std::env::var("CROWCLAW_LUNA_PROBE_PROFILE").expect("Explicit profile required"),
    );
    let output = std::path::PathBuf::from(
        std::env::var("CROWCLAW_LUNA_PROBE_OUTPUT")
            .expect("Explicit private receipt path required"),
    );
    let phase = std::env::var("CROWCLAW_LUNA_PROBE_PHASE").unwrap_or_else(|_| "fields".into());
    assert!(
        matches!(
            phase.as_str(),
            "fields"
                | "constraints"
                | "tools"
                | "large_input"
                | "maximum_output"
                | "long_output"
                | "reasoning_none"
        ),
        "Unknown probe phase"
    );
    assert!(
        !output.exists(),
        "Do not overwrite/replay a previous probe receipt"
    );
    let storage =
        Arc::new(Storage::open(&profile).expect("Could not open explicitly selected profile"));
    let service = Arc::new(
        MembershipService::new(storage).expect("Could not initialize native membership service"),
    );
    let accounts: Vec<_> = service
        .accounts()
        .expect("Account metadata unavailable")
        .into_iter()
        .filter(|a| a.identity.provider == "chatgpt" && a.has_credentials)
        .collect();
    assert_eq!(
        accounts.len(),
        1,
        "An unambiguous connected ChatGPT account is required"
    );
    let account = service
        .refresh_catalog(&accounts[0].id)
        .await
        .expect("Could not validate the selected account catalog");
    assert!(
        account
            .catalog
            .as_ref()
            .unwrap()
            .models
            .iter()
            .any(|m| m.slug == MODEL && m.reasoning_efforts.iter().any(|e| e == "low")),
        "Luna/low must be offered to this account"
    );
    let mut cases = vec![("baseline".to_string(), base())];
    if phase == "reasoning_none" {
        cases.clear();
        let mut payload = base();
        payload["reasoning"] = json!({"effort":"none"});
        payload["max_output_tokens"] = json!(64);
        cases.push((phase.clone(), payload));
    } else if matches!(
        phase.as_str(),
        "large_input" | "maximum_output" | "long_output"
    ) {
        cases.clear();
        let mut payload = base();
        payload["max_output_tokens"] = json!(128_000);
        if phase == "large_input" {
            let repeats = std::env::var("CROWCLAW_LUNA_PROBE_REPEATS")
                .ok()
                .map(|s| s.parse::<usize>().expect("Invalid repeat count"))
                .unwrap_or(900_000);
            assert!((1..=922_000).contains(&repeats));
            payload["max_output_tokens"] = json!(32);
            payload["input"][0]["content"] = json!(format!("Synthetic padding follows. Ignore it and reply only CONTEXT-PROBE-OK.\n{}\nReply only CONTEXT-PROBE-OK.", " a".repeat(repeats)));
        } else if phase == "long_output" {
            payload["input"][0]["content"] = json!("This is an explicitly authorized output-capacity test. Output integers 1 through 50000 in order separated by single spaces. No code, ellipses, summaries or explanations. Continue until all numbers are emitted or the output token limit is reached. Do not use tools.");
        }
        cases.push((phase.clone(), payload));
    } else if phase == "fields" {
        for (name, value) in [
            ("max_output_tokens", json!(32)),
            ("temperature", json!(0.2)),
            ("top_p", json!(0.5)),
            ("metadata", json!({"probe":"synthetic"})),
            ("background", json!(false)),
            ("max_tool_calls", json!(1)),
            ("stream", json!(false)),
        ] {
            let mut payload = base();
            payload[name] = value;
            cases.push((name.into(), payload));
        }
        let mut capped = base();
        capped["max_output_tokens"] = json!(4);
        capped["input"][0]["content"] =
            json!("List the integers from 1 through 20, separated by spaces. No tools.");
        cases.push(("output_cap_4".into(), capped));
    } else if phase == "constraints" {
        for limit in [4, 16, 32] {
            let mut payload = base();
            payload["max_output_tokens"] = json!(limit);
            payload["input"][0]["content"]=json!("List integers 1 through 100 separated by spaces. No tools, explanations or omissions.");
            cases.push((format!("long_output_cap_{limit}"), payload));
        }
        for (name, value) in [("temperature", json!(1.0)), ("top_p", json!(1.0))] {
            let mut payload = base();
            payload[name] = value;
            cases.push((format!("default_{name}"), payload));
        }
    } else {
        let function = json!({"type":"function","name":"echo_probe","description":"Synthetic local echo; no files, commands or network.","parameters":{"type":"object","properties":{"text":{"type":"string","enum":["PROBE"]}},"required":["text"],"additionalProperties":false},"strict":true});
        let mut flat = base();
        flat["tools"] = json!([function.clone()]);
        flat["tool_choice"] = json!("required");
        flat["input"][0]["content"] = json!("Call echo_probe once with text PROBE.");
        cases.push(("flat_function".into(), flat));
        let mut namespace = base();
        namespace["tools"] = json!([{"type":"namespace","name":"crowclaw_probe","description":"Synthetic local tests.","tools":[function]}]);
        namespace["tool_choice"] = json!("required");
        namespace["input"][0]["content"] =
            json!("Call crowclaw_probe.echo_probe once with text PROBE.");
        cases.push(("namespace_function".into(), namespace));
        let mut custom = base();
        custom["tools"] = json!([{"type":"namespace","name":"crowclaw_probe","description":"Synthetic local tests.","tools":[{"type":"custom","name":"echo_probe","description":"Return exactly PROBE as input. This does not execute code.","format":{"type":"text"}}]}]);
        custom["tool_choice"] = json!("required");
        custom["input"][0]["content"] = json!("Call crowclaw_probe.echo_probe with exactly PROBE.");
        cases.push(("namespace_custom".into(), custom));
        let mut search = base();
        search["tools"] = json!([{"type":"web_search"}]);
        search["tool_choice"] = json!("required");
        search["input"][0]["content"] =
            json!("Search for the official OpenAI website. Reply with its domain only.");
        cases.push(("web_search".into(), search));
    }
    let mut reports = Vec::new();
    for (name, payload) in cases {
        let observed = match probe(&service, &account.id, &name, payload.clone()).await {
            Ok(value) => value,
            Err(_) => {
                reports.push(json!({"case":name,"transportFailure":true}));
                save(&output, &reports).unwrap();
                break;
            }
        };
        println!(
            "Luna probe {}: HTTP {}, status {}",
            name, observed.report["http"], observed.report["status"]
        );
        let stop = quota_stop(&observed.report)
            || (name == "baseline" && observed.report["status"] != "completed");
        reports.push(observed.report.clone());
        save(&output, &reports).unwrap();
        if stop {
            break;
        }
        if name == "namespace_function" && observed.report["status"] == "completed" {
            if let Some(call) = observed
                .items
                .iter()
                .find(|item| item["type"] == "function_call" && item["name"] == "echo_probe")
            {
                let args = call["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok());
                if args.as_ref().is_some_and(|a| a["text"] == "PROBE") {
                    let mut next = payload;
                    next["tool_choice"] = json!("none");
                    let input = next["input"].as_array_mut().unwrap();
                    input.extend(observed.items.clone());
                    input.push(json!({"type":"function_call_output","call_id":call["call_id"],"output":"PROBE"}));
                    input.push(json!({"role":"user","content":"Reply exactly TOOL-PROBE-OK now that the local echo returned."}));
                    if let Ok(followup) =
                        probe(&service, &account.id, "namespace_function_followup", next).await
                    {
                        let stop = quota_stop(&followup.report);
                        reports.push(followup.report);
                        save(&output, &reports).unwrap();
                        if stop {
                            break;
                        }
                    }
                }
            }
        }
    }
    assert!(!reports.is_empty(), "No probe evidence produced");
}
