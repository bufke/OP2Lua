#pragma once

// campaign.* (Campaign.cpp): plain data a campaign mission saves for the next one, in a file beside the mission DLL.

#include <sol/sol.hpp>

namespace OP2Lua::Campaign {

// Makes the `campaign` table (save, load, delete).
void Register(sol::state& lua);

}  // namespace OP2Lua::Campaign
