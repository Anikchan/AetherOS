//! P9-T06 (partial) — input fuzzing for the package manifest parser.
//!
//! Deterministic pseudo-random text is fed to the TOML manifest parser. The
//! parser must never panic: it either returns a manifest or an error. This is
//! not a substitute for a full fuzzing campaign, but it is run on every test
//! run and covers the parser the installer and store trust.

use aether_pkg::manifest::parse_manifest;

/// Small deterministic LCG so the corpus is reproducible in CI.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }

    fn below(&mut self, limit: u64) -> u64 {
        if limit == 0 {
            0
        } else {
            self.next() % limit
        }
    }
}

fn random_text(lcg: &mut Lcg, len: usize) -> String {
    const ALPHABET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 \t\n\"'[]=.,:-_/\\{}#`$%^&*()";
    let mut text = String::with_capacity(len);
    for _ in 0..len {
        let index = lcg.below(ALPHABET.len() as u64) as usize;
        text.push(ALPHABET[index] as char);
    }
    text
}

#[test]
fn manifest_parser_never_panics_on_random_input() {
    let mut lcg = Lcg(0x5eed_1234_abcd_9876);
    for _ in 0..20_000 {
        let len = lcg.below(400) as usize;
        let text = random_text(&mut lcg, len);
        let _ = parse_manifest(&text);
    }
}

#[test]
fn manifest_parser_handles_structured_mutations() {
    let base = "id = \"org.example.App\"\nname = \"App\"\nversion = \"1.0.0\"\nkind = \"native-bundle\"\nentry = \"bin/app\"\ninstalled_bytes = 10\n";
    let mut lcg = Lcg(0xdead_beef_0000_0001);
    for _ in 0..5_000 {
        let mut text = base.to_string();
        let position = lcg.below(text.len() as u64) as usize;
        let alphabet = b" []\"'\n=:.\\/{}";
        let byte = alphabet[lcg.below(alphabet.len() as u64) as usize] as char;
        text.insert(position, byte);
        let _ = parse_manifest(&text);
    }
}

#[test]
fn extreme_lengths_are_rejected_cleanly() {
    let long = "x".repeat(10_000);
    let text = format!("id = \"{long}\"\nname = \"x\"\nversion = \"1\"\nkind = \"native-bundle\"\nentry = \"x\"\ninstalled_bytes = 1\n");
    assert!(parse_manifest(&text).is_err());
    assert!(parse_manifest(&"(".repeat(10_000)).is_err());
}
