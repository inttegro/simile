use snapbox::cmd::Command;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

fn fixture(name: &str) -> String {
    format!(
        "{}/../simile-core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn registry_home() -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    std::env::temp_dir().join(format!(
        "simile-cli-registry-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn remote_service_configuration() -> (String, thread::JoinHandle<()>) {
    let openapi =
        fs::read_to_string(fixture("run-orders.yaml")).unwrap_or_else(|error| panic!("{error}"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("{error}"));
    let config = format!(
        "service \"orders\" {{\n  listen {{ port = 4343 }}\n  openapi {{ url = \"http://{address}/openapi.yaml\" }}\n}}\n"
    );
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap_or_else(|error| panic!("{error}"));
            let mut request = [0_u8; 2048];
            let read = stream
                .read(&mut request)
                .unwrap_or_else(|error| panic!("{error}"));
            let request = String::from_utf8_lossy(&request[..read]);
            let (content_type, body) = if request.contains("GET /config.hcl ") {
                ("text/plain", config.as_str())
            } else {
                ("application/yaml", openapi.as_str())
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap_or_else(|error| panic!("{error}"));
        }
    });
    (format!("http://{address}/config.hcl"), server)
}

#[test]
fn prints_version_as_json() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args(["--output", "json", "version"])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"version\": \"0.2.0\",\n",
            "  \"architecture\": \"[..]\",\n",
            "  \"operatingSystem\": \"[..]\"\n",
            "}\n"
        ));
}

#[test]
fn doctor_writes_machine_output_only_to_stdout() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args(["--output", "json", "doctor"])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"ok\": true,\n",
            "  \"checks\": [\n",
            "    {\n",
            "      \"name\": \"build\",\n",
            "      \"status\": \"pass\",\n",
            "      \"detail\": \"simile 0.2.0\"\n",
            "    },\n",
            "    {\n",
            "      \"name\": \"current-directory\",\n",
            "      \"status\": \"pass\",\n",
            "      \"detail\": \"[..]\"\n",
            "    }\n",
            "  ]\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn compiles_a_service_contract_as_json() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--output",
            "json",
            "compile",
            &fixture("orders.yaml"),
            "--behavior",
            &fixture("orders.hcl"),
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"Orders\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.2.1\",\n",
            "  \"operations\": 1,\n",
            "  \"openapiDefaults\": 0,\n",
            "  \"hclBehaviors\": 1,\n",
            "  \"asyncOperations\": 1,\n",
            "  \"resources\": 1,\n",
            "  \"faults\": 0,\n",
            "  \"faultProfiles\": 0,\n",
            "  \"heuristicPolicy\": \"standard\"\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn start_rejects_a_name_that_does_not_match_the_service_configuration() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "start",
            "payments",
            "--config",
            &fixture("service-orders.hcl"),
        ])
        .assert()
        .failure()
        .stderr_eq("service configuration names `orders`, but `payments` was requested\n");
}

#[test]
fn start_rejects_port_zero_before_reading_the_configuration() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "start",
            "orders",
            "--config",
            "does-not-exist.hcl",
            "--port",
            "0",
        ])
        .assert()
        .failure()
        .stderr_eq(concat!(
            "error: invalid value '0' for '--port <PORT>': 0 is not in 1..=65535\n",
            "\n",
            "For more information, try '--help'.\n"
        ));
}

#[test]
fn registers_a_service_in_simile_home() {
    let home = registry_home();
    drop(fs::remove_dir_all(&home));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "--output",
            "json",
            "register",
            "orders",
            "--config",
            &fixture("service-orders.hcl"),
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"orders\",\n",
            "  \"revision\": \"[..]\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.1.2\",\n",
            "  \"listen\": \"127.0.0.1:4242\",\n",
            "  \"operations\": 3,\n",
            "  \"resources\": 1,\n",
            "  \"asyncOperations\": 1,\n",
            "  \"directory\": \"[..]\",\n",
            "  \"config\": \"[..]/config.hcl\",\n",
            "  \"artifact\": \"[..]/service.similec\"\n",
            "}\n"
        ))
        .stderr_eq("");

    assert!(home.join("layout-version").is_file());
    assert!(home.join("services/orders/current").is_file());

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "register",
            "orders",
            "--config",
            &fixture("service-orders.hcl"),
        ])
        .assert()
        .failure()
        .stderr_eq("service `orders` is already registered; pass --force to replace it\n");

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "register",
            "orders",
            "--config",
            &fixture("service-orders.hcl"),
            "--force",
        ])
        .assert()
        .success();

    fs::remove_dir_all(home).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn describes_a_transient_service_from_the_compiled_artifact() {
    let output = Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--output",
            "json",
            "--verbose",
            "describe",
            "service",
            "orders",
            "--config",
            &fixture("service-described.hcl"),
        ])
        .output()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(report["service"], "orders");
    assert_eq!(
        report["description"],
        "An emulator for order integration tests."
    );
    assert_eq!(report["operations"][0]["key"], "GET /health");
    assert!(
        report["operations"]
            .as_array()
            .is_some_and(|operations| operations.iter().any(|operation| {
                operation["operationId"] == "createOrder"
                    && operation["description"] == "Create and accept an order."
            }))
    );
    assert_eq!(
        report["scenarios"][0]["description"],
        "Return a valid order response slowly."
    );
    assert_eq!(
        report["faults"][0]["description"],
        "Delay the completed response before delivery."
    );
    assert_eq!(
        report["resources"][0]["description"],
        "Carries accepted orders to background processors."
    );
}

#[test]
fn registers_a_service_from_a_remote_configuration() {
    let home = registry_home();
    drop(fs::remove_dir_all(&home));
    let (config_url, server) = remote_service_configuration();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "--output",
            "json",
            "register",
            "orders",
            "--config",
            &config_url,
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"orders\",\n",
            "  \"revision\": \"[..]\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.1.2\",\n",
            "  \"listen\": \"127.0.0.1:4343\",\n",
            "  \"operations\": 3,\n",
            "  \"resources\": 0,\n",
            "  \"asyncOperations\": 0,\n",
            "  \"directory\": \"[..]\",\n",
            "  \"config\": \"[..]/config.hcl\",\n",
            "  \"artifact\": \"[..]/service.similec\"\n",
            "}\n"
        ));

    server
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
    fs::remove_dir_all(home).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn removes_only_an_exactly_confirmed_registered_service() {
    let home = registry_home();
    drop(fs::remove_dir_all(&home));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args(["remove", "orders"])
        .assert()
        .failure()
        .stderr_eq(concat!(
            "error: the following required arguments were not provided:\n",
            "  --confirm <SERVICE>\n",
            "\n",
            "Usage: simile[EXE] remove --confirm <SERVICE> <SERVICE>\n",
            "\n",
            "For more information, try '--help'.\n"
        ));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "register",
            "orders",
            "--config",
            &fixture("service-orders.hcl"),
        ])
        .assert()
        .success();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args(["remove", "orders", "--confirm", "payments"])
        .assert()
        .failure()
        .stderr_eq("service removal not confirmed; --confirm must exactly match `orders`\n");
    assert!(home.join("services/orders/current").is_file());

    let removed_directory = home.join("services").join("orders");
    let output = Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "--output",
            "json",
            "remove",
            "orders",
            "--confirm",
            "orders",
        ])
        .output()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report["service"], "orders");
    assert_eq!(
        report["directory"],
        removed_directory.to_string_lossy().as_ref()
    );

    assert!(!removed_directory.exists());
    assert!(home.join("layout-version").is_file());
    fs::remove_dir_all(home).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn updates_a_registered_service_from_a_remote_configuration() {
    let home = registry_home();
    drop(fs::remove_dir_all(&home));
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "register",
            "orders",
            "--config",
            &fixture("service-orders.hcl"),
        ])
        .assert()
        .success();

    let (config_url, server) = remote_service_configuration();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args([
            "--output",
            "json",
            "update",
            "orders",
            "--config",
            &config_url,
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"orders\",\n",
            "  \"revision\": \"[..]\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.1.2\",\n",
            "  \"listen\": \"127.0.0.1:4343\",\n",
            "  \"operations\": 3,\n",
            "  \"resources\": 0,\n",
            "  \"asyncOperations\": 0,\n",
            "  \"directory\": \"[..]\",\n",
            "  \"config\": \"[..]/config.hcl\",\n",
            "  \"artifact\": \"[..]/service.similec\"\n",
            "}\n"
        ));

    server
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
    let revision = fs::read_to_string(home.join("services/orders/current"))
        .unwrap_or_else(|error| panic!("{error}"));
    let manifest = fs::read_to_string(
        home.join("services/orders/revisions")
            .join(revision.trim())
            .join("manifest.json"),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(manifest.contains(&config_url));
    fs::remove_dir_all(home).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn start_reports_an_unregistered_service_without_config() {
    let home = registry_home();
    drop(fs::remove_dir_all(&home));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .env("SIMILE_HOME", &home)
        .args(["start", "orders"])
        .assert()
        .failure()
        .stderr_eq(concat!(
            "service `orders` is not registered; run `simile register orders --config URL_OR_FILE`\n"
        ));
}

#[test]
fn writes_and_inspects_a_compiled_artifact() {
    let artifact = std::env::temp_dir().join(format!(
        "simile-cli-artifact-{}.similec",
        std::process::id()
    ));
    drop(fs::remove_file(&artifact));
    let artifact_path = artifact.to_string_lossy().into_owned();
    let rendered_artifact_path = artifact_path.replace('\\', "/");

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "compile",
            &fixture("orders.yaml"),
            "--behavior",
            &fixture("orders.hcl"),
            "--artifact",
            &artifact_path,
        ])
        .assert()
        .success()
        .stdout_eq(format!(
            concat!(
                "Compiled Orders 1.0.0\n",
                "contract OpenAPI 3.2.1 · 1 operations\n",
                "behavior 0 OpenAPI defaults · 1 HCL overlays\n",
                "runtime 1 async operations · 1 resources\n",
                "faults 0 definitions · 0 profiles\n",
                "heuristics standard\n",
                "artifact {} · format 13 · [..] bytes\n",
                "digest sha256:[..]\n"
            ),
            rendered_artifact_path
        ));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args(["--output", "json", "inspect", &artifact_path])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"formatVersion\": 13,\n",
            "  \"compilerVersion\": \"0.2.0\",\n",
            "  \"service\": \"Orders\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.2.1\",\n",
            "  \"heuristicPolicy\": \"standard\",\n",
            "  \"operations\": 1,\n",
            "  \"requestBodies\": 0,\n",
            "  \"responseBodies\": 0,\n",
            "  \"schemaNodes\": 0,\n",
            "  \"heuristicRules\": 0,\n",
            "  \"responses\": 1,\n",
            "  \"outcomes\": 1,\n",
            "  \"providerAuthoredOutcomes\": 0,\n",
            "  \"scenarioSelections\": 0,\n",
            "  \"scenarioFaults\": 0,\n",
            "  \"faults\": 0,\n",
            "  \"faultProfiles\": 0,\n",
            "  \"schemaSynthesizedOutcomes\": 0,\n",
            "  \"underspecifiedOperations\": 0,\n",
            "  \"publicExportSafe\": true,\n",
            "  \"operationOutcomes\": [\n",
            "    {\n",
            "      \"operation\": \"POST /orders\",\n",
            "      \"outcomes\": [\n",
            "        {\n",
            "          \"name\": \"http_201\",\n",
            "          \"status\": 201,\n",
            "          \"provenance\": \"open_api_derived\"\n",
            "        }\n",
            "      ],\n",
            "      \"underspecified\": false\n",
            "    }\n",
            "  ],\n",
            "  \"valueNodes\": [..],\n",
            "  \"resources\": 1,\n",
            "  \"asyncOperations\": 1,\n",
            "  \"encodedBytes\": [..],\n",
            "  \"payloadSha256\": \"[..]\"\n",
            "}\n"
        ));

    fs::remove_file(artifact).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn runs_an_operation_by_id_with_a_synthesized_request() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--output",
            "json",
            "--verbose",
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"Orders\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"operation\": \"POST /orders\",\n",
            "  \"operationId\": \"createOrder\",\n",
            "  \"request\": {\n",
            "    \"source\": \"synthesized\",\n",
            "    \"contentType\": \"application/json\",\n",
            "    \"body\": {\n",
            "      \"product\": \"[..]\",\n",
            "      \"quantity\": [..]\n",
            "    }\n",
            "  },\n",
            "  \"response\": {\n",
            "    \"status\": 201,\n",
            "    \"contentType\": \"application/json\",\n",
            "    \"body\": {\n",
            "      \"id\": \"ord_simile\",\n",
            "      \"product\": \"[..]\",\n",
            "      \"quantity\": [..]\n",
            "    }\n",
            "  }\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn runs_an_operation_with_a_complete_request_object() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--output",
            "json",
            "--verbose",
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
            "--request",
            r#"{"headers":{"Content-Type":"application/json"},"body":{"product":"custom_jingle","quantity":4}}"#,
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"service\": \"Orders\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"operation\": \"POST /orders\",\n",
            "  \"operationId\": \"createOrder\",\n",
            "  \"request\": {\n",
            "    \"source\": \"provided\",\n",
            "    \"headers\": {\n",
            "      \"Content-Type\": \"application/json\"\n",
            "    },\n",
            "    \"contentType\": \"application/json\",\n",
            "    \"body\": {\n",
            "      \"product\": \"custom_jingle\",\n",
            "      \"quantity\": 4\n",
            "    }\n",
            "  },\n",
            "  \"response\": {\n",
            "    \"status\": 201,\n",
            "    \"contentType\": \"application/json\",\n",
            "    \"body\": {\n",
            "      \"id\": \"ord_simile\",\n",
            "      \"product\": \"custom_jingle\",\n",
            "      \"quantity\": 4\n",
            "    }\n",
            "  }\n",
            "}\n"
        ));
}

#[test]
fn reads_a_complete_request_object_from_a_bare_file_path() {
    let request =
        std::env::temp_dir().join(format!("simile-cli-request-{}.json", std::process::id()));
    fs::write(
        &request,
        r#"{"headers":{"Content-Type":"application/json"},"body":{"product":"licensed_beat","quantity":2}}"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let request_path = request.to_string_lossy().into_owned();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
            "--request",
            &request_path,
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"id\": \"ord_simile\",\n",
            "  \"product\": \"licensed_beat\",\n",
            "  \"quantity\": 2\n",
            "}\n"
        ));

    fs::remove_file(request).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn reads_a_complete_request_object_from_standard_input() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
            "--request",
            "-",
        ])
        .stdin(
            r#"{"headers":{"Content-Type":"application/json"},"body":{"product":"custom_jingle","quantity":3}}"#,
        )
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"id\": \"ord_simile\",\n",
            "  \"product\": \"custom_jingle\",\n",
            "  \"quantity\": 3\n",
            "}\n"
        ));
}

#[test]
fn prints_only_the_pretty_response_payload_by_default() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"id\": \"ord_simile\",\n",
            "  \"product\": \"[..]\",\n",
            "  \"quantity\": [..]\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn colors_the_default_response_payload_when_requested() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--color",
            "always",
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \u{1b}[34m\"id\"\u{1b}[0m: \u{1b}[32m\"ord_simile\"\u{1b}[0m,\n",
            "  \u{1b}[34m\"product\"\u{1b}[0m: \u{1b}[32m\"[..]\"\u{1b}[0m,\n",
            "  \u{1b}[34m\"quantity\"\u{1b}[0m: \u{1b}[36m[..]\u{1b}[0m\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn keeps_explicit_json_output_free_of_color() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "--output",
            "json",
            "--color",
            "always",
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"id\": \"ord_simile\",\n",
            "  \"product\": \"[..]\",\n",
            "  \"quantity\": [..]\n",
            "}\n"
        ))
        .stderr_eq("");
}

#[test]
fn runs_a_bodyless_operation_by_structural_key() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation",
            "GET /health",
        ])
        .assert()
        .success()
        .stdout_eq("null\n");
}

#[test]
fn synthesizes_an_optional_form_request() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "optionalForm",
            "--content-type",
            "application/x-www-form-urlencoded",
            "--verbose",
        ])
        .assert()
        .success()
        .stdout_eq(concat!(
            "Ran POST /optional (optionalForm)\n",
            "request synthesized · application/x-www-form-urlencoded\n",
            "{\"note\":\"[..]\"}\n",
            "response 204\n"
        ));
}

#[test]
fn reads_a_spec_url_without_a_path() {
    let source =
        fs::read_to_string(fixture("run-orders.yaml")).unwrap_or_else(|error| panic!("{error}"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("{error}"));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap_or_else(|error| panic!("{error}"));
        let mut request = [0_u8; 2048];
        let _ = stream
            .read(&mut request)
            .unwrap_or_else(|error| panic!("{error}"));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/yaml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            source.len(),
            source
        )
        .unwrap_or_else(|error| panic!("{error}"));
    });

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            "--spec-url",
            &format!("http://{address}/openapi.yaml"),
            "--operation",
            "GET /health",
        ])
        .assert()
        .success()
        .stdout_eq("null\n");
    server
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
}

#[test]
fn rejects_multiple_spec_sources_and_operation_selectors() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--spec-url",
            "https://example.com/openapi.yaml",
            "--operation",
            "GET /health",
        ])
        .assert()
        .failure();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation",
            "POST /orders",
            "--operation-id",
            "createOrder",
        ])
        .assert()
        .failure();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
            "--body",
            "request.json",
        ])
        .assert()
        .failure();

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args([
            "run",
            &fixture("run-orders.yaml"),
            "--operation-id",
            "createOrder",
            "--request",
            "request.json",
            "--content-type",
            "application/json",
        ])
        .assert()
        .failure();
}
