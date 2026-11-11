use std::{fs, path::Path, process::Command};

use serde_json::Value;

mod support;

#[test]
fn investigation_preserves_batch_evidence_and_gaps() {
    let root = support::temp_directory("investigate-batch");
    fs::create_dir_all(&root).unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let profile = examples.join("batch-profile.json");
    let complete = fs::read_to_string(examples.join("batch-shortfall.log")).unwrap();
    let final_count = "batch=A17 final stage=confirmed count=9790\n";
    assert!(complete.contains(final_count));
    let cases = [
        ("complete", complete.clone(), "difference"),
        (
            "missing",
            complete.replace(final_count, ""),
            "insufficient_evidence",
        ),
        (
            "conflict",
            format!("{complete}batch=A17 final stage=confirmed count=9780\n"),
            "insufficient_evidence",
        ),
    ];

    for (case, log, expected) in cases {
        let input = root.join("input.log");
        fs::write(&input, log).unwrap();
        let imported = Command::new(env!("CARGO_BIN_EXE_runsift"))
            .args(["import", "--case-id", case, "--output"])
            .arg(&root)
            .arg(&input)
            .output()
            .unwrap();
        assert!(imported.status.success(), "{:?}", imported.stderr);

        let output = Command::new(env!("CARGO_BIN_EXE_runsift"))
            .arg("investigate")
            .arg(root.join(case))
            .args(["--question", "Why is A17 short?", "--profile"])
            .arg(&profile)
            .args(["--", "sh", "-c", r#"
                case "$(cat)" in
                    *'"status":"insufficient_evidence"'*)
                        printf '%s' '{"action":"finish","findings":[],"gaps":["Need a reliable confirmed count"],"next_step":"Check final acknowledgement"}' ;;
                    *'"status":"difference"'*)
                        printf '%s' '{"action":"finish","findings":[],"gaps":["Physical loss is unverified"],"next_step":"Compare output records"}' ;;
                    *)
                        printf '%s' '{"action":"check_batch","batch_id":"A17"}' ;;
                esac
            "#])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let result = &report["trace"][0]["result"];
        assert_eq!(report["status"], "completed");
        assert_eq!(result["status"], expected);
        match case {
            "complete" => {
                assert_eq!(result["first_difference"]["difference"], 10);
                assert_eq!(result["first_difference"]["to_stage"], "confirmed");
            }
            "missing" => assert_eq!(result["missing_stages"][0], "confirmed"),
            _ => assert_eq!(
                result["stages"][2]["conflicting_observation"]["count"],
                9780
            ),
        }
    }
    fs::remove_dir_all(root).unwrap();
}
