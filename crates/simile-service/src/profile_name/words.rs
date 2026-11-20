//! Stable words used for generated profile names.
//!
//! Words stay lowercase, ASCII, and safe for profile names, shell output, URLs,
//! and filenames. Keep additions familiar and neutral so generated names remain
//! easy to read aloud and recognize in logs.

pub(super) const ADJECTIVES: &[&str] = &[
    "agile", "amber", "ample", "brave", "bright", "brisk", "calm", "candid", "clear", "clever",
    "cool", "crisp", "eager", "early", "fair", "fast", "fine", "fresh", "gentle", "glad", "grand",
    "happy", "hardy", "honest", "jolly", "kind", "lively", "lucid", "mellow", "merry", "mighty",
    "modern", "neat", "nimble", "noble", "open", "patient", "plain", "proud", "quick", "quiet",
    "rapid", "ready", "robust", "safe", "sharp", "simple", "smart", "steady", "still", "strong",
    "sunny", "swift", "tidy", "true", "vivid", "warm", "wise", "young",
];

pub(super) const NOUNS: &[&str] = &[
    "acorn", "alder", "atlas", "beacon", "birch", "breeze", "brook", "cedar", "cloud", "coast",
    "comet", "coral", "dawn", "delta", "dune", "elm", "ember", "field", "finch", "forest", "fox",
    "grove", "harbor", "heron", "hill", "island", "juniper", "kite", "lake", "lark", "maple",
    "meadow", "moon", "oak", "ocean", "orbit", "owl", "pine", "plume", "river", "robin", "sage",
    "shore", "sky", "sparrow", "star", "stone", "summit", "tide", "trail", "vale", "wave",
    "willow", "wren",
];
