// hackaray.* - an opt-in bridge to Hackaray.dll, the community game-patching library some missions ship with
// (in their zip). Only the calls such a mission makes: the nuclear-missile patch and the
// end-of-game videos.
//
// A mission must ask for the library with its own line, `hackaray.load()`. Until it does, OP2Lua never looks for
// Hackaray.dll, and the other calls raise an error that says so. load() is LoadLibraryA("Hackaray.dll"), as the
// original missions do, so the DLL must sit in every player's game folder. Whatever a mission switched on is
// switched off again, and the library freed, when the mission is unloaded (Reset), as those missions'
// OnUnloadMission does.
//
// The game-rule patch (nuke_patch) changes how every machine plays: in a multiplayer game every player needs the
// same Hackaray.dll, or the games desync. The videos are local display only.

#include "Hackaray.h"

#include <windows.h>

#include <algorithm>
#include <string>

#include "Log.h"

using std::max;  // Tethys/API/Game.h uses max and min unqualified, as Bindings.cpp sets up
using std::min;
#include "Tethys/API/Game.h"
#include "Tethys/API/Player.h"

namespace OP2Lua::Hackaray {
namespace {

using SetNukePatchFn = bool(__cdecl*)(bool on);
using SetMissionEndHook2Fn = void(__cdecl*)(bool on, const char* winAvi, const char* loseAvi);

struct State {
  bool asked = false;  // the mission called hackaray.load()
  HMODULE dll = nullptr;
  SetNukePatchFn setNukePatch = nullptr;
  SetMissionEndHook2Fn setMissionEndHook2 = nullptr;
  bool nukeOn = false;
  bool videosOn = false;
};
State s;

bool Loaded() { return s.dll && s.setNukePatch && s.setMissionEndHook2; }

// For the calls after load(): an error when the mission never asked for the library; false when it asked and the
// library isn't there.
bool Ready(const char* call) {
  if (!s.asked) throw sol::error(std::string("hackaray.") + call + ": the mission must call hackaray.load() first");
  return Loaded();
}

}  // namespace

void Register(sol::state& lua) {
  sol::table h = lua.create_named_table("hackaray");
  // hackaray.load() -> true when Hackaray.dll is in the game folder with SetNukePatch and SetMissionEndHook2. The only
  // call that looks for the library; once per mission (later calls give the same answer).
  h.set_function("load", [] {
    if (!s.asked) {
      s.asked = true;
      s.dll = LoadLibraryA("Hackaray.dll");
      if (s.dll) {
        s.setNukePatch = reinterpret_cast<SetNukePatchFn>(GetProcAddress(s.dll, "SetNukePatch"));
        s.setMissionEndHook2 = reinterpret_cast<SetMissionEndHook2Fn>(GetProcAddress(s.dll, "SetMissionEndHook2"));
      }
      if (Loaded()) {
        LOG_MISSION(INFO) << "hackaray: Hackaray.dll loaded";
      } else {
        LOG_MISSION(WARNING) << "hackaray: Hackaray.dll " << (s.dll ? "lacks SetNukePatch or SetMissionEndHook2" : "not found")
                             << " (it goes in the game folder)";
      }
    }
    return Loaded();
  });
  // hackaray.nuke_patch(on) -> true when the patch is on (or off, for false): the Spaceport can build and launch the
  // nuclear missile. False when Hackaray.dll is missing or refused the patch.
  h.set_function("nuke_patch", [](bool on) {
    if (!Ready("nuke_patch")) return false;
    const bool ok = s.setNukePatch(on);
    if (ok) s.nukeOn = on;
    LOG_MISSION(INFO) << "hackaray: nuke patch " << (on ? "on" : "off") << (ok ? "" : " refused");
    return ok;
  });
  // hackaray.end_videos(win_avi, lose_avi) -> true when set: the videos the game plays when the mission is won or lost
  // (e.g. "CES.avi", "MPUW.avi"); end_videos() with no names turns them off. False when Hackaray.dll is missing.
  // end_videos{ eden = { win, lose }, plymouth = { win, lose } } picks the pair by the colony of the player at this
  // machine (display only: the scripts never see which player is local).
  h.set_function("end_videos", [](sol::object first, sol::optional<std::string> second) {
    if (!Ready("end_videos")) return false;
    sol::optional<std::string> win, lose;
    if (first.is<sol::table>()) {
      const bool eden = Tethys::TethysAPI::_Player(Tethys::TethysAPI::Game::LocalPlayer()).IsEden();
      sol::optional<sol::table> pair = first.as<sol::table>()[eden ? "eden" : "plymouth"];
      if (pair) {
        win = (*pair)[1].get<sol::optional<std::string>>();
        lose = (*pair)[2].get<sol::optional<std::string>>();
      }
    } else if (first.is<std::string>()) {
      win = first.as<std::string>();
      lose = second;
    }
    const bool on = win.has_value() && lose.has_value();
    s.setMissionEndHook2(on, on ? win->c_str() : nullptr, on ? lose->c_str() : nullptr);
    s.videosOn = on;
    LOG_MISSION(INFO) << "hackaray: end videos " << (on ? *win + ", " + *lose : std::string("off"));
    return true;
  });
}

void Reset() {
  if (s.dll) {
    if (s.nukeOn && s.setNukePatch) s.setNukePatch(false);
    if (s.videosOn && s.setMissionEndHook2) s.setMissionEndHook2(false, nullptr, nullptr);
    FreeLibrary(s.dll);
    LOG(INFO) << "hackaray: patches switched off, Hackaray.dll freed";
  }
  s = State();
}

}  // namespace OP2Lua::Hackaray
