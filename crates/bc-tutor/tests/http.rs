use std::time::Duration;

use bc_tutor::{
    ChatMessage, ChatRequest, LabRequest, Learner, Provider, Tutor, TutorConfig, TutorError,
    ZoneRequest,
};
use serde_json::{json, Value};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn guide_json() -> String {
    json!({
        "summary": "Introduces opportunity cost.",
        "can_do_alone": [{ "idea": "Trade-offs", "because": "everyday choices" }],
        "zone": [{
            "concept": "Opportunity cost",
            "why_it_matters": "It is the lens for the rest of the chapter.",
            "hints": ["What did you give up?", "Think of choosing a film.", "It's the best option you didn't take."],
            "explanation": "The value of the best alternative forgone.",
            "try_it": "What is the opportunity cost of an hour of reading?",
            "answer": "Whatever else you would most have done with that hour."
        }],
        "not_yet": [],
        "check": [{ "question": "Define opportunity cost.", "answer": "Best alternative forgone." }],
        "next_step": "Look for examples as you read on."
    })
    .to_string()
}

fn req() -> ZoneRequest {
    ZoneRequest {
        title: "Economics".into(),
        location: "page 3".into(),
        passage: "Every choice has a cost: the next best thing you give up.".into(),
        learner: Learner {
            stretch: 3,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn tutor(server: &MockServer, provider: Provider, model: &str) -> Tutor {
    let base = match provider {
        Provider::Anthropic => server.uri(),
        Provider::OpenaiCompatible => format!("{}/v1", server.uri()),
    };
    Tutor::new(
        reqwest::Client::new(),
        TutorConfig {
            provider,
            model: model.into(),
            base_url: base,
        },
        "sk-test",
    )
    .with_retry_base(Duration::from_millis(1))
}

#[tokio::test]
async fn anthropic_request_shape_and_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test"))
        .and(header("anthropic-version", "2026-10-21"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(body["model"], "claude-opus-5-5");
            assert_eq!(body["output_config"]["format"]["type"], "json_schema");
            assert!(body.get("temperature").is_none());
            // Custom base URL: no server-side fallback.
            assert!(body.get("fallbacks").is_none());
            let user = body["messages"][0]["content"].as_str().unwrap();
            assert!(user.contains("Every choice has a cost"));
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "claude-opus-5-5",
                "stop_reason": "end_turn",
                "content": [
                    { "type": "thinking", "thinking": "" },
                    { "type": "text", "text": guide_json() }
                ]
            }))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = tutor(&server, Provider::Anthropic, "")
        .generate(&req())
        .await
        .unwrap();
    assert_eq!(out.model, "claude-opus-5-5");
    assert_eq!(out.guide.zone[0].concept, "Opportunity cost");
}

#[tokio::test]
async fn anthropic_refusal_and_truncation() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "stop_reason": "refusal",
            "stop_details": { "type": "refusal", "category": null, "explanation": "policy" },
            "content": []
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "stop_reason": "max_tokens",
            "content": [{ "type": "text", "text": "{\"summary\": \"cut" }]
        })))
        .mount(&server)
        .await;
    let t = tutor(&server, Provider::Anthropic, "claude-opus-5-5");
    assert!(matches!(t.generate(&req()).await, Err(TutorError::Refused(m)) if m == ": policy"));
    assert!(matches!(
        t.generate(&req()).await,
        Err(TutorError::Truncated)
    ));
}

#[tokio::test]
async fn retries_overload_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(529).set_body_json(json!({
            "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" }
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": guide_json() }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    assert!(tutor(&server, Provider::Anthropic, "")
        .generate(&req())
        .await
        .is_ok());
}

#[tokio::test]
async fn bad_key_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let e = tutor(&server, Provider::Anthropic, "")
        .generate(&req())
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "tutor_auth");
}

#[tokio::test]
async fn openai_compatible_shape_and_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header_exists("authorization"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(body["model"], "local-model");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["response_format"]["json_schema"]["strict"], true);
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "local-model",
                "choices": [{
                    "finish_reason": "stop",
                    "message": { "role": "assistant", "content": format!("```json\n{}\n```", guide_json()) }
                }]
            }))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = tutor(&server, Provider::OpenaiCompatible, "local-model")
        .generate(&req())
        .await
        .unwrap();
    assert_eq!(out.guide.check.len(), 1);
}

#[tokio::test]
async fn openai_ping_sends_no_token_cap() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            // Newer OpenAI models return 400 for `max_tokens`.
            assert!(body.get("max_tokens").is_none());
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "gpt",
                "choices": [{ "finish_reason": "stop", "message": { "content": "OK" } }]
            }))
        })
        .expect(1)
        .mount(&server)
        .await;
    tutor(&server, Provider::OpenaiCompatible, "gpt")
        .ping()
        .await
        .unwrap();
}

#[tokio::test]
async fn ping_reports_missing_model() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "type": "error", "error": { "type": "not_found_error", "message": "model: nope" }
        })))
        .mount(&server)
        .await;
    let e = tutor(&server, Provider::Anthropic, "nope")
        .ping()
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "tutor_model");
}

fn labs_json() -> String {
    json!({
        "overview": "Practice the book.",
        "audience": "Beginners.",
        "prerequisites": [],
        "tools_overview": "A laptop.",
        "labs": [{
            "title": "First lab", "objective": "Do the thing.", "difficulty": "beginner",
            "estimated_minutes": 20, "source_refs": ["Chapter 1"], "concepts": ["Addresses"],
            "setup_requirements": ["A laptop"], "setup_steps": ["Turn it on."],
            "outcomes": ["Run the command"], "theory": "Addresses first.", "deliverable": "Command output.",
            "steps": [{ "title": "Run it", "instructions": "Run the command.", "code": "ip a", "why": "Practice.", "expected": "Output.", "tip": "" }],
            "diagrams": [{ "title": "Setup", "kind": "topology", "mermaid": "graph LR\n  A[Laptop] --> B[Router]", "explanation": "One link." }],
            "checks": [{ "question": "Did it work?", "answer": "Yes, if output appeared." }],
            "troubleshooting": [], "extension": ""
        }],
        "capstone": "", "safety_notes": []
    })
    .to_string()
}

#[tokio::test]
async fn anthropic_labs_shape_and_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(body["output_config"]["format"]["type"], "json_schema");
            let user = body["messages"][0]["content"].as_str().unwrap();
            assert!(user.contains("<book_text>"));
            assert!(user.contains("Every device needs an address"));
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "claude-opus-5-5", "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": labs_json() }]
            }))
        })
        .expect(1)
        .mount(&server)
        .await;
    let req = LabRequest {
        title: "Networking".into(),
        excerpt: "Every device needs an address...".into(),
        learner: Learner {
            stretch: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    let out = tutor(&server, Provider::Anthropic, "")
        .generate_labs(&req)
        .await
        .unwrap();
    assert_eq!(out.guide.labs[0].title, "First lab");
    assert_eq!(out.guide.labs[0].all_diagrams().len(), 1);
    assert_eq!(out.guide.labs[0].steps[0].code, "ip a");
    assert!(out
        .guide
        .to_markdown("Networking", None)
        .contains("```mermaid"));
    let empty = LabRequest {
        excerpt: "  ".into(),
        ..req
    };
    assert!(tutor(&server, Provider::Anthropic, "")
        .generate_labs(&empty)
        .await
        .is_err());
}

fn chat_req() -> ChatRequest {
    ChatRequest {
        title: "Economics".into(),
        location: "page 3".into(),
        context: "Every choice has a cost.".into(),
        history: vec![
            ChatMessage::user("What is opportunity cost?"),
            ChatMessage::assistant("The best alternative you give up."),
            ChatMessage::user("Give me an example."),
        ],
        ..Default::default()
    }
}

#[tokio::test]
async fn anthropic_chat_stream_shape_and_deltas() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"An hour \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"of reading.\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(body["stream"], true);
            let roles: Vec<&str> = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["role"].as_str().unwrap())
                .collect();
            // Context merges into the first user turn; turns alternate.
            assert_eq!(roles, vec!["user", "assistant", "user"]);
            assert!(body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Every choice"));
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        })
        .expect(1)
        .mount(&server)
        .await;
    let mut deltas = Vec::new();
    let out = tutor(&server, Provider::Anthropic, "")
        .chat_stream(&chat_req(), |d| {
            deltas.push(d.to_string());
            true
        })
        .await
        .unwrap();
    assert_eq!(out.text, "An hour of reading.");
    assert_eq!(out.model, "claude-opus-5-5");
    assert!(!out.truncated);
    assert_eq!(deltas.join(""), out.text);
}

#[tokio::test]
async fn openai_chat_stream_and_cancel() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"model\":\"local-model\",\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(body["stream"], true);
            assert_eq!(body["messages"][0]["role"], "system");
            assert!(body.get("max_tokens").is_none());
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        })
        .expect(2)
        .mount(&server)
        .await;
    let t = tutor(&server, Provider::OpenaiCompatible, "local-model");
    let out = t.chat_stream(&chat_req(), |_| true).await.unwrap();
    assert_eq!(out.text, "Hello");
    // Cancelling on the first delta: no retry after output (expect(2)
    // total over both calls proves it).
    let e = t.chat_stream(&chat_req(), |_| false).await.unwrap_err();
    assert!(matches!(e, TutorError::Cancelled));
    assert_eq!(e.kind(), "cancelled");
}

#[tokio::test]
async fn stream_falls_back_to_plain_json_reply() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "local-model",
            "choices": [{ "finish_reason": "stop", "message": { "content": "Whole reply." } }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut deltas = Vec::new();
    let out = tutor(&server, Provider::OpenaiCompatible, "local-model")
        .chat_stream(&chat_req(), |d| {
            deltas.push(d.to_string());
            true
        })
        .await
        .unwrap();
    assert_eq!(out.text, "Whole reply.");
    assert_eq!(deltas, vec!["Whole reply."]);
}

#[tokio::test]
async fn chat_stream_retries_before_first_delta() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(529).set_body_json(json!({
            "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" }
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            ),
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;
    let out = tutor(&server, Provider::Anthropic, "")
        .chat_stream(&chat_req(), |_| true)
        .await
        .unwrap();
    assert_eq!(out.text, "OK");
}

#[tokio::test]
async fn chat_error_mapping_and_local_validation() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "type": "error", "error": { "type": "not_found_error", "message": "model: nope" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let t = tutor(&server, Provider::Anthropic, "nope");
    let e = t.chat(&chat_req()).await.unwrap_err();
    assert_eq!(e.kind(), "tutor_model");
    // Empty / malformed history is refused before any network call.
    let empty = ChatRequest::default();
    assert_eq!(t.chat(&empty).await.unwrap_err().kind(), "not_configured");
    let mut no_user_end = chat_req();
    no_user_end.history = vec![ChatMessage::assistant("hi")];
    assert_eq!(
        t.chat_stream(&no_user_end, |_| true)
            .await
            .unwrap_err()
            .kind(),
        "not_configured"
    );
}

#[tokio::test]
async fn empty_passage_is_rejected_locally() {
    let server = MockServer::start().await;
    let r = ZoneRequest {
        passage: "   ".into(),
        ..req()
    };
    assert!(tutor(&server, Provider::Anthropic, "")
        .generate(&r)
        .await
        .is_err());
}
