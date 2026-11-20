//! Gives the Windows executable its icon, and tells the About box which versions of its main libraries it was
//! built with (read from Cargo.lock).

const LIBRARIES: &[&str] = &["eframe", "op2-formats", "rodio", "rfd", "image", "regex", "ab_glyph"];

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=assets/app.ico");

    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let mut found = Vec::new();
    let mut name = None;
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = \"") {
            name = Some(n.trim_end_matches('"').to_string());
        } else if let (Some(v), Some(n)) = (line.strip_prefix("version = \""), name.take())
            && LIBRARIES.contains(&n.as_str())
            && !found.iter().any(|(f, _): &(String, String)| *f == n)
        {
            found.push((n, v.trim_end_matches('"').to_string()));
        }
    }
    found.sort_by_key(|(n, _)| LIBRARIES.iter().position(|l| l == n));
    let list: Vec<String> = found.iter().map(|(n, v)| format!("{n} {v}")).collect();
    println!("cargo:rustc-env=OP2LUAEDITOR_LIBRARIES={}", list.join(";"));

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("FileDescription", "OP2LuaEditor");
        res.set("ProductName", "OP2LuaEditor");
        if let Err(e) = res.compile() {
            // No resource compiler: the editor still builds, without its icon.
            println!("cargo:warning=no icon in the executable: {e}");
        }
    }
}
