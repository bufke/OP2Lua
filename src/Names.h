#pragma once

#include <string>

#include "Tethys/Game/MapObjectType.h"  // Tethys::MapID

namespace OP2Lua {

// Map a type name (e.g. "CommandCenter", "Tiger", "Microwave") to its MapID; MapID::None if unknown.
Tethys::MapID MapIdFromName(const std::string& name);

// Reverse: MapID -> type name (e.g. for unit.type). Empty string if unknown.
std::string NameFromMapId(Tethys::MapID id);

// The weapon `carrier` mounts for `weapon`. A Guard Post (or a ConVec's Guard Post kit) carries the turret versions of
// the self-destruct weapons: "Starflare" and "Supernova" name StarflareGP (0x43) and SupernovaGP (0x44) on it, as the
// stock missions pass them, and the vehicle versions (0x41, 0x42) on anything else. A Guard Post made with the
// vehicle version faults the game as soon as it is drawn. Every other weapon is the same on both.
Tethys::MapID WeaponFor(Tethys::MapID carrier, Tethys::MapID weapon);

// Direction name ("East".."NorthEast") -> UnitDirection integer 0..7 (default 0/East).
int DirFromName(const std::string& name);

// Sound name (e.g. "Savant4", "Beep2") -> SoundID integer; -1 if unknown.
int SoundIdFromName(const std::string& name);

// Cargo type name ("Food", "CommonOre", "RareMetal"...) -> CargoType integer; -1 if unknown.
int CargoTypeFromName(const std::string& name);

// Reverse: CargoType integer -> name (e.g. for unit.cargo). "Empty" / "" if unknown.
std::string NameFromCargoType(int cargoType);

}  // namespace OP2Lua
