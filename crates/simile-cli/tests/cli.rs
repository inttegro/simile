use snapbox::cmd::Command;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn fixture(name: &str) -> String {
    format!(
        "{}/../simile-core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn prints_version_as_json() {
    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args(["--output", "json", "version"])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"version\": \"0.1.0\",\n",
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
            "      \"detail\": \"simile 0.1.0\"\n",
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
            "  \"resources\": 1\n",
            "}\n"
        ))
        .stderr_eq("");
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
                "artifact {} · format 6 · [..] bytes\n",
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
            "  \"formatVersion\": 6,\n",
            "  \"compilerVersion\": \"0.1.0\",\n",
            "  \"service\": \"Orders\",\n",
            "  \"serviceVersion\": \"1.0.0\",\n",
            "  \"openapiVersion\": \"3.2.1\",\n",
            "  \"operations\": 1,\n",
            "  \"requestBodies\": 0,\n",
            "  \"responseBodies\": 0,\n",
            "  \"schemaNodes\": 0,\n",
            "  \"responses\": 1,\n",
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
            "  },\n",
            "  \"asyncOperations\": []\n",
            "}\n"
        ))
        .stderr_eq("");
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
            "response 204\n",
            "async none\n"
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
}
