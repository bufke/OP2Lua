#pragma once

// Mission runtime (OP2LuaCore). The exported entry points are the OP2LuaCore_* C API in
// OP2LuaCore.h; this header just exposes the teardown used by the Lua host.

#include <cstddef>
#include <string>
#include <vector>

namespace OP2Lua::Mission {

// When this OP2LuaCore.dll was built (its link time, local "YYYY-MM-DD HH:MM:SS"): which build is running, for the
// log and game.version().
std::string BuildTime();

// The version the running game goes by (TApp::Version: "1.2.0.7" for the GOG exe, "1.4.0.1" under OPU 1.4.x), and
// Outpost2.exe's own file version ("1.2.0.7"). Empty when not found.
std::string GameVersion();
std::string GameFileVersion();

// The folder the running mission's DLL is in (where its scripts and its own files are).
std::string Dir();
// Clear the active mission and release its callback + placement references. Must run while the
// Lua state is still alive (callbacks hold refs into it).
void Reset();

// Saved games (SaveGame.cpp hooks the game's save and load): the script's state as the save region's bytes (a script
// whose state can't be saved gets a region that says why; false with `err` would fail the save), and the state put back once the game has loaded a save's triggers and groups (a save
// whose state can't be put back loads with the script stopped, and says why). LoadedWithoutState: a save made before
// the core kept the state.
bool BuildSave(std::vector<char>& out, std::string& err);
void RestoreSave(const char* data, size_t size);
void LoadedWithoutState();

}  // namespace OP2Lua::Mission
