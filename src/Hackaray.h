#pragma once

// hackaray.* (Hackaray.cpp): the opt-in bridge to Hackaray.dll's nuke patch and end-of-game videos.

#include <sol/sol.hpp>

#include "Persist.h"

namespace OP2Lua::Hackaray {

// Makes the `hackaray` table. Hackaray.dll is looked for only when a mission calls hackaray.load().
void Register(sol::state& lua);

// Switches off what the mission switched on and frees the library (the mission is unloaded or replaced).
void Reset();

// Saved games: what the mission switched on, and switching it on again after a load.
void SaveState(Persist::Writer& w);
void RestoreState(Persist::Reader& r);

}  // namespace OP2Lua::Hackaray
