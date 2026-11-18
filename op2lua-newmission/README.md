# op2lua-newmission

A tiny **cross-platform** console tool that scaffolds a new OP2Lua mission: it clones the prebuilt
mission stub (`LuaMission.dll`), byte-patches its menu name, map, type and player count, and writes starter
`<Mission>.lua` + `<Mission>.placement.lua` next to it. No game compiler involved.

It's the portable equivalent of the SDK's `tools/new-mission.ps1` (which stays for quick Windows
use), and the manual stand-in until the OP2MissionEditor `.opm` -> Lua converter lands.

Pure C++17 (`<filesystem>` + `<fstream>`) - no OS-specific APIs. Builds on Windows, Linux, macOS.

## Build

```
cmake -B build
cmake --build build --config Release
```

The binary is `op2lua-newmission` (`.exe` on Windows). A prebuilt **Windows** binary
(`op2lua-newmission.exe`) ships in the **OP2LuaSDK** under `tools/`, so authors don't have to build it.
The SDK's **Linux** build (`op2lua-newmission`, x86-64 static) is older: it has `--map` only and names the
DLL without the `c`. On Linux, `chmod +x op2lua-newmission` first.

### Cross-compiling the Linux binary from Windows

No Linux box needed - [`zig`](https://ziglang.org) bundles a full cross toolchain:

```
zig c++ -target x86_64-linux-musl -std=c++17 -O2 -Wl,--strip-all main.cpp -o op2lua-newmission
```

The `musl` target produces a fully static ELF that runs on any Linux distro.

## Use

```
op2lua-newmission "My First Mission"
op2lua-newmission "Red Dawn" --map sgame01.map --out ./missions
op2lua-newmission "Test" --stub ../bin/LuaMission.dll
op2lua-newmission "Duel" --map on2_02.map --players 2 --type LastOneStanding
```

| Option | Meaning |
|---|---|
| `--map <file>` | map the mission uses (default `eden01.map`) |
| `--players <1-6>` | players in the mission (default 2); patched into the DLL's `DescBlock` |
| `--type <type>` | `Colony` (default), or a multiplayer type: `LandRush`, `SpaceRace`, `ResourceRace`, `Midas`, `LastOneStanding`; patched into the `DescBlock`, and OP2 lists the mission under it |
| `--out <dir>` | output directory (default `./out`) |
| `--stub <path>` | path to `LuaMission.dll` (default: auto-detected near the cwd / executable) |
| `-h`, `--help` | usage |

It writes three files (base name = the mission name with non-alphanumerics removed):

```
c<Base>.dll             the mission OP2 lists under Colony Games (menu name + map byte-patched)
<Base>.lua              starter logic (win/lose already wired)
<Base>.placement.lua    starter layout
```

The core finds `c<Base>.dll`'s scripts as `<Base>.*` by dropping a `c` that comes before a capital, so a
base that doesn't start with a capital (`convoyrun`) keeps the `c` on the scripts too (`cconvoyrun.lua`).
A multiplayer `--type` writes `<Base>.dll` with no `c` (the lobby lists it), a placement without a
`players` table (the lobby sets the colonies) and a mission that starts the type's victory.

Copy all three into your Outpost 2 `OPU/maps` folder and launch the game. See the SDK's
`INSTALL.md` and `docs/GETTING-STARTED.md`.

## Finding the stub

`LuaMission.dll` is searched for (in order): `--stub`, `./LuaMission.dll`, `./bin/LuaMission.dll`,
then beside the executable and in `../bin/` relative to it (the SDK layout: `tools/op2lua-newmission`
+ `bin/LuaMission.dll`). Pass `--stub` if it lives somewhere else.
