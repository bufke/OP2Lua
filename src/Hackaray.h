#pragma once

// hackaray.* (Hackaray.cpp): the opt-in bridge to Hackaray.dll's nuke patch and end-of-game videos.

#include <sol/sol.hpp>

namespace OP2Lua::Hackaray {

// Makes the `hackaray` table. Hackaray.dll is looked for only when a mission calls hackaray.load().
void Register(sol::state& lua);

// Switches off what the mission switched on and frees the library (the mission is unloaded or replaced).
void Reset();

}  // namespace OP2Lua::Hackaray
