use aether_skills::{admit_skill, install_skill, SkillLoader, SkillPinStore};
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden_harness/fixtures/skills/poisoned")
}

#[test]
fn poisoned_corpus_has_zero_install_escapes() {
    let corpus: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root().join("corpus.json")).unwrap()).unwrap();
    let mut escapes = 0u32;
    for case in corpus["cases"].as_array().unwrap() {
        let kind = case["kind"].as_str().unwrap();
        if kind == "install_and_execute" {
            let skill = SkillLoader::load_from_file(
                &root()
                    .join(case["skill_dir"].as_str().unwrap())
                    .join("SKILL.md"),
            )
            .unwrap();
            let mut pins = SkillPinStore::new();
            install_skill(&mut pins, &skill).expect("benign install");
            admit_skill(&pins, &skill).expect("benign admit");
            continue;
        }
        if kind == "rug_pull" {
            let base_dir = case["skill_dir"].as_str().unwrap();
            let base =
                SkillLoader::load_from_file(&root().join(base_dir).join("SKILL.md")).unwrap();
            let mut_text = fs::read_to_string(
                root()
                    .join(case["mutated_skill_dir"].as_str().unwrap())
                    .join("SKILL.md"),
            )
            .unwrap();
            let mutated =
                SkillLoader::parse(&mut_text, &root().join(base_dir).join("SKILL.md")).unwrap();
            let mut pins = SkillPinStore::new();
            install_skill(&mut pins, &base).unwrap();
            let err = admit_skill(&pins, &mutated).unwrap_err().to_string();
            assert!(err.contains("pin mismatch"), "{err}");
            continue;
        }
        let skill = SkillLoader::load_from_file(
            &root()
                .join(case["skill_dir"].as_str().unwrap())
                .join("SKILL.md"),
        )
        .unwrap();
        let mut pins = SkillPinStore::new();
        match install_skill(&mut pins, &skill) {
            Ok(_) => {
                eprintln!("escape: {}", case["id"]);
                escapes += 1;
            }
            Err(e) => {
                let needle = case["expected_reject_contains"].as_str().unwrap();
                assert!(
                    e.to_string()
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase()),
                    "{}: expected {needle:?} in {e}",
                    case["id"]
                );
            }
        }
    }
    assert_eq!(escapes, 0);
}
