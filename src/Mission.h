#pragma once

// Mission runtime (OP2LuaCore). The exported entry points are the OP2LuaCore_* C API in
// OP2LuaCore.h; this header just exposes the teardown used by the Lua host.

#include <string>

namespace OP2Lua::Mission {

// When this OP2LuaCore.dll was built (its link time, local "YYYY-MM-DD HH:MM:SS"): which build is running, for the
// log and game.version().
std::string BuildTime();

// The version the running game goes by (TApp::Version: "1.2.0.7" for the GOG exe, "1.4.0.1" under OPU 1.4.x), and
// Outpost2.exe's own file version ("1.2.0.7"). Empty when not found.
std::string GameVersion();
std::string GameFileVersion();

// Clear the active mission and release its callback + placement references. Must run while the
// Lua state is still alive (callbacks hold refs into it).
void Reset();

}  // namespace OP2Lua::Mission
