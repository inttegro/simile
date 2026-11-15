use snapbox::cmd::Command;
use std::fs;

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
                "artifact {} · format 4 · [..] bytes\n",
                "digest sha256:[..]\n"
            ),
            artifact_path
        ));

    Command::new(snapbox::cmd::cargo_bin!("simile"))
        .args(["--output", "json", "inspect", &artifact_path])
        .assert()
        .success()
        .stdout_eq(concat!(
            "{\n",
            "  \"formatVersion\": 4,\n",
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
