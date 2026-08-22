//! Fingerprints the dungeon layout generator: layouts never travel the wire
//! (both sides generate them from the entrance id), so a client built before a
//! generator change draws a maze the server does not have. See the layout
//! fingerprint section of doc/REMOTE_AGENT_CLIENT.md.

include!("src/fnv.rs");

/// Excluded so a test-only edit does not reload the whole fleet.
const SKIP: &str = "tests.rs";

fn main() {
    // Spelled as `/`-joined strings rather than `PathBuf`s on purpose. The path
    // bytes are hashed, and `read_dir("src/dungeon")` hands its entries back as
    // `src/dungeon\gen.rs` on Windows — so a Windows build fingerprinted the
    // same sources differently from the Linux server, and every handshake it
    // made was refused for a dungeon layout that was in fact identical. Windows
    // accepts `/` in std::fs paths, so one spelling serves both the hash and
    // the read below.
    let mut inputs = vec![String::from("../data-src/dungeons.csv")];
    println!("cargo:rerun-if-changed=src/dungeon");
    let dir = std::fs::read_dir("src/dungeon").expect("shared/src/dungeon");
    inputs.extend(
        dir.map(|e| e.expect("dungeon dir entry").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".rs") && name.as_str() != SKIP)
            .map(|name| format!("src/dungeon/{name}")),
    );
    inputs.sort();

    let mut hash = FNV_OFFSET;
    for path in &inputs {
        println!("cargo:rerun-if-changed={path}");
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("layout input {path:?}: {e}"));
        // Path included, so moving code between these files still counts.
        hash = fnv1a64(hash, path.bytes());
        // CR dropped: the Windows agent-client build must hash the same source.
        hash = fnv1a64(hash, bytes.into_iter().filter(|b| *b != b'\r'));
    }
    println!("cargo:rustc-env=LAYOUT_VERSION={hash:016x}");
}
