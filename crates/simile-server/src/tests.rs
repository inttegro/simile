use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::Duration;

use super::*;
use crate::access_log::AccessLine;
use crate::inspection::{sanitize_body, sanitize_parameters};
use crate::response::{operation_execution_response, validation_error_message};
use crate::routing::{Match, RouteTable};
use serde_json::json;
use simile_compiler::{CompileRequest, Compiler, Source};
use simile_heuristics::Policy;
use simile_runtime::Runtime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

fn artifact() -> Artifact {
    let source = r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              additionalProperties: false
              required: [customer, items]
              properties:
                customer:
                  type: object
                  required: [email]
                  properties:
                    email: { type: string }
                items:
                  type: array
                  minItems: 1
                  items:
                    type: object
                    required: [quantity]
                    properties:
                      quantity: { type: integer, minimum: 1 }
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
        '400':
          description: invalid order request
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [code, message]
                properties:
                  code: { const: invalid_order }
                  message: { const: order request rejected }
  /orders/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema: { type: string, pattern: '^ord_' }
    get:
      operationId: getOrder
      parameters:
        - name: limit
          in: query
          schema: { type: integer, minimum: 1 }
        - name: X-Trace
          in: header
          schema: { type: string, minLength: 3 }
      responses:
        '200':
          description: found
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
  /orders/recent:
    get:
      operationId: recentOrders
      responses:
        '204': { description: empty }
  /file-links/{id}/open:
    get:
      operationId: openFileLink
      responses:
        '303':
          description: open file
          headers:
            Location:
              schema: { type: string, format: uri }
              example: https://files.example.test/download/file_1234
"#;
    compile_artifact(source)
}

fn artifact_without_client_error() -> Artifact {
    compile_artifact(
        r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [customer]
              properties:
                customer: { type: string }
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
"#,
    )
}

fn artifact_with_faults() -> Artifact {
    compile_artifact_with_behavior(
        r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        '201':
          description: created
          content:
            application/json:
              schema:
                type: object
                required: [id]
                properties:
                  id: { type: string }
"#,
        r#"
fault "slow_response" {
  phase = "before_response"
  delay { milliseconds = 20 }
}

fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}

fault_profile "always_slow" {
  probability       = 1
  faults            = ["slow_response"]
  operations        = ["createOrder"]
  max_consecutive   = 1
  cooldown_requests = 1
  seed              = 7
}

scenario "slow" {
  inject "createOrder" { fault = "slow_response" }
}

scenario "lost" {
  inject "createOrder" { fault = "response_lost" }
}
"#,
    )
}

fn artifact_with_authentication() -> Artifact {
    compile_artifact_with_behavior(
        r#"
openapi: 3.1.0
info: { title: Secured orders, version: 1.0.0 }
paths:
  /private:
    get:
      operationId: privateOrders
      security:
        - merchantKey: []
      responses:
        '200':
          description: allowed
          content:
            application/json:
              schema:
                type: object
                required: [allowed]
                properties:
                  allowed: { const: true }
components:
  securitySchemes:
    merchantKey:
      type: apiKey
      in: header
      name: X-Merchant-Key
"#,
        r#"
principal "merchant" {
  credential "merchantKey" {}
}

profile "merchant" {
  principal = principal.merchant
  credential "merchantKey" { value = "sim_test_merchant" }
}
"#,
    )
}

fn compile_artifact(source: &str) -> Artifact {
    compile_artifact_with_behavior(source, "")
}

fn compile_artifact_with_behavior(source: &str, behavior: &str) -> Artifact {
    let compiled = Compiler::new()
        .compile(CompileRequest {
            openapi: Source::new("orders.yml", source),
            modules: (!behavior.is_empty())
                .then(|| Source::new("faults.hcl", behavior))
                .into_iter()
                .collect(),
            heuristic_policy: Policy::Standard,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let encoded = simile_artifact::encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
    simile_artifact::decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn formats_a_single_safe_access_log_line() {
    let method = Method::POST;
    let line = AccessLine {
        method: &method,
        path: "/orders",
        status: StatusCode::CREATED,
        origin_ip: Some(IpAddr::from([127, 0, 0, 1])),
        user_agent: Some("simile-test/1.0"),
        request_bytes: Some(47),
        request_content_type: Some("application/json"),
        response_bytes: Some(58),
        response_content_type: Some("application/json"),
        elapsed: Duration::from_micros(321),
    };

    assert_eq!(
        line.to_string(),
        "POST /orders 201 origin=127.0.0.1 user-agent=\"simile-test/1.0\" request=47B \
             request-type=\"application/json\" response=58B response-type=\"application/json\" \
             elapsed=321us"
    );
}

#[test]
fn formats_validation_error_summaries() {
    assert_eq!(
        validation_error_message(1),
        "request body is invalid; fix the issue listed in `issues` and retry"
    );
    assert_eq!(
        validation_error_message(3),
        "request body is invalid; fix the 3 issues listed in `issues` and retry"
    );
}

#[test]
fn prefers_literal_routes_and_reports_allowed_methods() {
    let artifact = artifact();
    let routes = RouteTable::new(&artifact);

    assert!(matches!(
        routes.find(&Method::GET, "/orders/recent"),
        Match::Operation { operation: "GET /orders/recent", path } if path.is_empty()
    ));
    assert!(matches!(
        routes.find(&Method::GET, "/orders/ord_123"),
        Match::Operation { operation: "GET /orders/{id}", path }
            if path.get("id") == Some(&vec!["ord_123".to_owned()])
    ));
    assert!(matches!(
        routes.find(&Method::POST, "/orders/ord_123"),
        Match::MethodNotAllowed(methods) if methods == BTreeSet::from(["GET"])
    ));
    assert!(matches!(
        routes.find(&Method::GET, "/customers/cus_123"),
        Match::NotFound
    ));
}

#[tokio::test]
async fn binds_to_an_ephemeral_loopback_port() {
    let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact())
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_ne!(server.local_addr().port(), 0);
    assert!(server.local_addr().ip().is_loopback());
}

#[tokio::test]
async fn rejects_an_unknown_scenario_before_binding() {
    let result = Server::bind_with_scenario(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        artifact(),
        Some("missing".to_owned()),
    )
    .await;

    assert!(matches!(result, Err(Error::UnknownScenario(name)) if name == "missing"));
}

#[tokio::test]
async fn rejects_an_unknown_fault_profile_before_binding() {
    let result = Server::bind_with_options(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        artifact_with_faults(),
        ServerOptions {
            scenario: None,
            fault_profile: Some("missing".to_owned()),
            reporting_sender: None,
        },
    )
    .await;

    assert!(matches!(result, Err(Error::UnknownFaultProfile(name)) if name == "missing"));
}

#[tokio::test]
async fn reports_the_active_fault_profiles_seed() {
    let server = Server::bind_with_options(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        artifact_with_faults(),
        ServerOptions {
            scenario: None,
            fault_profile: Some("always_slow".to_owned()),
            reporting_sender: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(server.fault_seed(), Some(7));
}

#[tokio::test]
async fn enacts_delay_and_disconnect_transport_plans() {
    let artifact = artifact_with_faults();
    let runtime = Runtime::new(&artifact);
    let call = OperationCall {
        operation: "POST /orders",
        content_type: None,
        body: b"",
    };
    let delayed = runtime
        .execute_with_options(
            call,
            ExecutionOptions {
                scenario: Some("slow"),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let started = Instant::now();
    let response = operation_execution_response(delayed, false).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(started.elapsed() >= Duration::from_millis(10));

    let disconnected = runtime
        .execute_with_options(
            call,
            ExecutionOptions {
                scenario: Some("lost"),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let response = operation_execution_response(disconnected, false).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(to_bytes(response.into_body(), 1_024).await.is_err());
}

async fn serve_raw_with_artifact(artifact: Artifact, request: &[u8]) -> String {
    let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let address = server.local_addr();
    let (shutdown, signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run_until(async {
        drop(signal.await);
    }));
    let response = send_raw(address, request).await;
    let _ = shutdown.send(());
    task.await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| panic!("{error}"));
    response
}

async fn send_raw(address: SocketAddr, request: &[u8]) -> String {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    stream
        .write_all(request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    String::from_utf8(response).unwrap_or_else(|error| panic!("{error}"))
}

async fn serve_raw(request: &[u8]) -> String {
    serve_raw_with_artifact(artifact(), request).await
}

#[tokio::test]
async fn serves_a_request_through_the_operation_runtime() {
    let response =
        serve_raw(b"GET /orders/ord_123 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await;

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("content-type: application/json"));
    assert!(response.contains(r#""id":"#));
}

#[tokio::test]
async fn exposes_live_inspection_only_to_loopback_browser_origins() {
    let artifact = artifact();
    let artifact_digest = artifact.summary().payload_sha256;
    let server = Server::bind(SocketAddr::from(([127, 0, 0, 1], 0)), artifact)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let address = server.local_addr();
    let (shutdown, signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run_until(async {
        drop(signal.await);
    }));

    let body = br#"{"customer":{"email":"ada@example.test"},"items":[{"quantity":1}]}"#;
    let request = format!(
        "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    );
    let operation = send_raw(address, request.as_bytes()).await;
    assert!(operation.starts_with("HTTP/1.1 201 Created\r\n"));

    let inspection = send_raw(
        address,
        b"GET /.simile/inspection HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(inspection.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(inspection.contains(&format!(r#""artifactDigest":"{artifact_digest}""#)));

    let exchanges = send_raw(
            address,
            b"GET /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4175\r\nConnection: close\r\n\r\n",
        )
        .await;
    assert!(exchanges.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(exchanges.contains("access-control-allow-origin: http://127.0.0.1:4175\r\n"));
    assert!(exchanges.contains(r#""operation":"POST /orders""#));
    assert!(exchanges.contains(r#""status":201"#));
    assert!(exchanges.contains(r#""email":"ada@example.test""#));

    let state = send_raw(
        address,
        b"GET /.simile/state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(state.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(state.contains(r#""resources":"#));
    assert!(state.contains(r#""events":"#));

    let deferred = send_raw(
        address,
        b"GET /.simile/deferred HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(deferred.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(deferred.contains(r#""pending":"#));
    assert!(deferred.contains(r#""deadLetters":"#));

    let unconfirmed_reset = send_raw(
            address,
            b"POST /.simile/state/reset HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .await;
    assert!(unconfirmed_reset.starts_with("HTTP/1.1 422 Unprocessable Entity\r\n"));
    assert!(unconfirmed_reset.contains("reset_confirmation_required"));

    let reset = send_raw(
            address,
            b"POST /.simile/state/reset HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"confirm\":true}",
        )
        .await;
    assert!(reset.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(reset.contains(r#""reset":true"#));

    let reset_state = send_raw(
        address,
        b"GET /.simile/state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(reset_state.contains(r#""events":[]"#));

    let cleared = send_raw(
            address,
            b"DELETE /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4175\r\nConnection: close\r\n\r\n",
        )
        .await;
    assert!(cleared.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(cleared.contains(r#""removed":1"#));

    let empty_exchanges = send_raw(
        address,
        b"GET /.simile/exchanges HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(empty_exchanges.contains(r#""exchanges":[]"#));

    let forbidden = send_raw(
            address,
            b"GET /.simile/inspection HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\nConnection: close\r\n\r\n",
        )
        .await;
    assert!(forbidden.starts_with("HTTP/1.1 403 Forbidden\r\n"));
    assert!(forbidden.contains("inspection_origin_forbidden"));

    let _ = shutdown.send(());
    task.await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn does_not_expose_inspection_on_a_non_loopback_listener() {
    let server = Server::bind(SocketAddr::from(([0, 0, 0, 0], 0)), artifact())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let address = SocketAddr::from(([127, 0, 0, 1], server.local_addr().port()));
    let (shutdown, signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run_until(async {
        drop(signal.await);
    }));

    let response = send_raw(
        address,
        b"GET /.simile/inspection HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));

    let _ = shutdown.send(());
    task.await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn serves_declared_redirect_headers() {
    let response = serve_raw(
        b"GET /file-links/link_123/open HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 303 See Other\r\n"));
    assert!(response.contains("location: https://files.example.test/download/file_1234\r\n"));
}

#[tokio::test]
async fn enforces_declared_authentication_before_operation_execution() {
    let missing = serve_raw_with_artifact(
        artifact_with_authentication(),
        b"GET /private HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(missing.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
    assert!(missing.contains(r#""code":"authentication_required""#));

    let accepted = serve_raw_with_artifact(
            artifact_with_authentication(),
            b"GET /private HTTP/1.1\r\nHost: localhost\r\nX-Merchant-Key: sim_test_merchant\r\nConnection: close\r\n\r\n",
        )
        .await;
    assert!(accepted.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(accepted.contains(r#""allowed":true"#));
}

#[tokio::test]
async fn validates_path_query_and_header_parameters_before_execution() {
    let invalid = serve_raw(
            b"GET /orders/not-an-order?limit=0 HTTP/1.1\r\nHost: localhost\r\nX-Trace: ok\r\nConnection: close\r\n\r\n",
        )
        .await;
    let (_, body) = invalid
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response should contain a body"));
    let payload: serde_json::Value =
        serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert_eq!(payload["error"]["code"], "invalid_parameters");
    assert_eq!(payload["error"]["issues"].as_array().map(Vec::len), Some(3));

    let valid = serve_raw(
            b"GET /orders/ord_123?limit=2 HTTP/1.1\r\nHost: localhost\r\nX-Trace: trace-1\r\nConnection: close\r\n\r\n",
        )
        .await;
    assert!(valid.starts_with("HTTP/1.1 200 OK\r\n"));
}

#[tokio::test]
async fn returns_the_service_declared_validation_error() {
    let body = br#"{"customer":{},"items":[{"quantity":0}],"debug":true}"#;
    let request = format!(
        "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    );
    let response = serve_raw(request.as_bytes()).await;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response should contain headers and a body"));
    let payload: serde_json::Value =
        serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

    assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert_eq!(
        payload,
        json!({
            "code": "invalid_order",
            "message": "order request rejected"
        })
    );
}

#[tokio::test]
async fn falls_back_to_simile_diagnostics_without_a_declared_client_error() {
    let body = br#"{}"#;
    let request = format!(
        "POST /orders HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    );
    let response =
        serve_raw_with_artifact(artifact_without_client_error(), request.as_bytes()).await;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response should contain headers and a body"));
    let payload: serde_json::Value =
        serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}"));

    assert!(response.starts_with("HTTP/1.1 422 Unprocessable Entity\r\n"));
    assert_eq!(payload["error"]["code"], "invalid_body");
    assert_eq!(
        payload["error"]["issues"],
        json!([{
            "path": "/customer",
            "rule": "required",
            "message": "required property `customer` is missing"
        }])
    );
}

#[test]
fn sanitizes_sensitive_incident_evidence_without_flattening_the_request() {
    let body = br#"{
          "customer": {"name": "Ada", "api_key": "sk_live_secret"},
          "password_reset_token": "reset-secret",
          "items": [{"reference": "sku_123"}]
        }"#;
    let sanitized = sanitize_body(Some("application/problem+json; charset=utf-8"), body)
        .unwrap_or_else(|| panic!("JSON incident body must be sanitizable"));
    let value: serde_json::Value = serde_json::from_slice(&sanitized)
        .unwrap_or_else(|error| panic!("sanitized body must remain JSON: {error}"));

    assert_eq!(value["customer"]["name"], "Ada");
    assert_eq!(value["customer"]["api_key"], "<redacted>");
    assert_eq!(value["password_reset_token"], "<redacted>");
    assert_eq!(value["items"][0]["reference"], "sku_123");

    let parameters = sanitize_parameters(IncomingParameters {
        path: BTreeMap::from([("access_token".to_owned(), vec!["path-secret".to_owned()])]),
        query: BTreeMap::from([("page".to_owned(), vec!["2".to_owned()])]),
        headers: BTreeMap::from([("authorization".to_owned(), vec!["Bearer secret".to_owned()])]),
        cookies: BTreeMap::from([("session_token".to_owned(), vec!["cookie".to_owned()])]),
    });
    assert_eq!(parameters.path["access_token"], ["<redacted>"]);
    assert_eq!(parameters.query["page"], ["2"]);
    assert_eq!(parameters.headers["authorization"], ["<redacted>"]);
    assert_eq!(parameters.cookies["session_token"], ["<redacted>"]);
}

#[test]
fn declines_opaque_request_bodies_for_automatic_reports() {
    assert!(sanitize_body(Some("application/octet-stream"), b"secret bytes").is_none());
    assert!(sanitize_body(None, b"secret bytes").is_none());
}
