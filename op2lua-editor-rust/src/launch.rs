//! Test in OP2: copy the open mission into the game's `OPU\maps\LUA` and start Outpost 2 straight into it through
//! OPU's `op2launcher.exe`; and which OP2LuaCore the game has.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::dll;

/// Where Outpost 2 (OPU 1.4) loads Lua missions from.
pub fn deploy_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("OPU").join("maps").join("LUA")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A mission's files to copy into the game: (source, destination).
pub struct Deploy {
    /// Every file to copy, source and destination.
    pub copies: Vec<(PathBuf, PathBuf)>,
    /// Files in the game that differ from the mission's and weren't put there by this editor: may be another mission
    /// of the same name, so they are replaced only after asking.
    pub replaces: Vec<String>,
}

/// What Test in OP2 copies for the mission whose DLL is `dll_path` (none when it is in the game's folder already).
pub fn plan_deploy(game_dir: &Path, dll_path: &Path, base: &str, deployed: &HashSet<PathBuf>) -> Deploy {
    let dir = dll_path.parent().unwrap_or(Path::new("."));
    let target = deploy_dir(game_dir);
    let mut plan = Deploy { copies: Vec::new(), replaces: Vec::new() };
    if same_dir(dir, &target) {
        return plan;
    }
    let names = [
        dll_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        format!("{base}.lua"),
        format!("{base}.placement.lua"),
    ];
    for name in names {
        let (src, dst) = (dir.join(&name), target.join(&name));
        if !src.is_file() {
            continue;
        }
        if dst.is_file() && !deployed.contains(&dst) && std::fs::read(&src).ok() != std::fs::read(&dst).ok() {
            plan.replaces.push(name);
        }
        plan.copies.push((src, dst));
    }
    plan
}

/// Copies the files; each copied one is remembered, so the next test updates it without asking.
pub fn deploy(plan: &Deploy, deployed: &mut HashSet<PathBuf>) -> Result<(), String> {
    for (src, dst) in &plan.copies {
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        }
        std::fs::copy(src, dst).map_err(|e| format!("Can't copy {} to {}: {e}", src.display(), dst.display()))?;
        deployed.insert(dst.clone());
    }
    Ok(())
}

/// Starts Outpost 2 straight into `dll_name` with OPU's op2launcher.exe (run from the OPU folder, where it looks for
/// the mission and outpost2.ini). Elsewhere than Windows it runs under Wine.
pub fn launch(game_dir: &Path, dll_name: &str) -> Result<(), String> {
    let opu = game_dir.join("OPU");
    let launcher = opu.join("op2launcher.exe");
    if !launcher.is_file() {
        return Err(format!(
            "There is no op2launcher.exe in {}.\nCopy op2launcher.exe and Launcher.dll into the OPU folder.",
            opu.display()
        ));
    }
    let mut cmd = if cfg!(windows) {
        std::process::Command::new(&launcher)
    } else {
        let mut c = std::process::Command::new("wine");
        c.arg(&launcher);
        c
    };
    cmd.arg(dll_name).current_dir(&opu).spawn().map(|_| ()).map_err(|e| format!("Can't start {}: {e}", launcher.display()))
}

/// "OP2LuaCore 0.8.3, built 2026-11-19 09:47 UTC", or why there is none.
pub fn core_info(game_dir: &Path) -> (String, bool) {
    let core = game_dir.join("OPU").join("OP2LuaCore.dll");
    let Ok(bytes) = std::fs::read(&core) else { return ("OP2LuaCore: not installed".into(), false) };
    let version = dll::version_string(&bytes, "FileVersion").unwrap_or_else(|| "?".into());
    let built = dll::link_time(&bytes).map(|t| format!(", built {}", utc(t as i64))).unwrap_or_default();
    (format!("OP2LuaCore {version}{built}"), true)
}

/// Unix seconds as "YYYY-MM-DD HH:MM UTC".
fn utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(1_763_545_620), "2025-11-19 09:47 UTC");
    }

    #[test]
    fn deploy_plans_and_asks() {
        let root = std::env::temp_dir().join(format!("op2luaeditor-deploy-{}", std::process::id()));
        let (game, src) = (root.join("game"), root.join("work"));
        std::fs::create_dir_all(deploy_dir(&game)).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        for f in ["cA.dll", "A.lua", "A.placement.lua"] {
            std::fs::write(src.join(f), f).unwrap();
        }
        std::fs::write(deploy_dir(&game).join("A.lua"), "someone else's").unwrap();
        let mut deployed = HashSet::new();
        let plan = plan_deploy(&game, &src.join("cA.dll"), "A", &deployed);
        assert_eq!(plan.copies.len(), 3);
        assert_eq!(plan.replaces, vec!["A.lua".to_string()]);
        deploy(&plan, &mut deployed).unwrap();
        assert_eq!(std::fs::read_to_string(deploy_dir(&game).join("A.lua")).unwrap(), "A.lua");
        std::fs::write(src.join("A.lua"), "changed").unwrap();
        assert!(plan_deploy(&game, &src.join("cA.dll"), "A", &deployed).replaces.is_empty());
        // A mission already in the game's folder isn't copied.
        assert!(plan_deploy(&game, &deploy_dir(&game).join("cA.dll"), "A", &deployed).copies.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
