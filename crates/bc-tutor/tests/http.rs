use std::time::Duration;

use bc_tutor::{Learner, Provider, Tutor, TutorConfig, TutorError, ZoneRequest};
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
