#pragma once

// SaveGame - a Lua mission's state in the game's saved games.
//
// The script's state goes where a mission DLL's own data goes: the save region (GetSaveRegions), which the game writes
// in the save's mission block, its size first. OPU 1.4.1 tells a mission DLL neither that the game is being saved nor
// that a saved game was loaded, and the game takes the region's size once, when it loads the DLL. So the core hooks
// three spots of Outpost2.exe 1.2.0.7, all inside the mission block's own code:
//   - TethysGame's save (0x48A2D7, its call of MissionManager::Save 0x402C60): the state is written into the region
//     and the region's size set to it, just before the game writes them;
//   - MissionManager::Load's region size check (0x402E9A): a Lua mission's region takes the size the save has (the
//     game wants the size it took when it loaded the DLL);
//   - MissionManager::Load's call of ScStubList::Load (0x402ED6): with the region read and the game's triggers and
//     groups loaded, the state is put back (Mission::RestoreSave).
// A save made before the core kept the state has no region: its first word is a human player's default group, -1;
// the check steps back over it and the load goes on without one (Mission::LoadedWithoutState).

#include <string>
#include <vector>

namespace OP2Lua::SaveGame {

// Installs the hooks, once per process. False (logged once) when Outpost2.exe isn't the one they were made for: then
// a Lua mission gives the game no save region, as before.
bool Install();

// The save region a Lua mission gives the game (GetSaveRegions' argument, MissionManager +0x10). Called for every
// Lua mission DLL the game loads.
void UseRegion(void* pSaveRegion);

}  // namespace OP2Lua::SaveGame
