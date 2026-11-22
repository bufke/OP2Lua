// Mission.cpp - the OP2LuaCore mission runtime.
//
// Implements the OP2LuaCore_* C API (OP2LuaCore.h): load a mission folder's Lua, resolve its
// on_* callbacks, and run them when the per-mission stub DLL forwards the game's mission calls.
// All callbacks run inside protected calls so a script bug can never crash or desync the game.

#include "OP2LuaCore.h"  // OP2LUACORE_EXPORTS is defined by the build for this target

#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <exception>
#include <string>
#include <system_error>
#include <utility>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <filesystem>

#include "ApiTypes.h"
#include "Log.h"
#include "LuaHost.h"
#include "Mission.h"
#include "Placement.h"
#include "ErrorHandling.h"
#include "Guard.h"
#include "Hackaray.h"

#include "Tethys/API/Mission.h"
#include "Tethys/Game/GameStartInfo.h"   // MissionResults (OnEndMission)
#include "Tethys/Game/GameImpl.h"        // the game's end reason
#include "Tethys/API/Unit.h"

namespace fs = std::filesystem;
using namespace Tethys::TethysAPI;
using Tethys::ibool;

namespace OP2Lua::Mission {
namespace {

struct MissionState {
  bool active = false;
  sol::protected_function onInit, onTick, onChat, onCreateUnit, onDestroyUnit, onDamageUnit,
                          onLoad, onUnload, onEnd, onSave, onLoadSave;
  sol::protected_function pump;  // __op2lua_pump (scheduler), from the runtime preamble
};
MissionState g;
std::string gDir;   // the running mission's folder (Dir())
std::string gBase;  // the running mission DLL's name without .dll (its log and result file names)

void LogCbError(const char* cb, const std::string& what) {
  // A script error is a mission event - send it to the mission log, not the framework Main log.
  LOG_MISSION(ERROR) << "Unhandled Lua error in mission " << cb << "(): " << what;
}

// Case-insensitive string equality (map filenames may differ only in case, e.g. Eden01 vs eden01).
bool EqualsIgnoreCase(const std::string& a, const std::string& b) {
  if (a.size() != b.size()) return false;
  for (size_t i = 0; i < a.size(); ++i) {
    if (std::tolower(static_cast<unsigned char>(a[i])) != std::tolower(static_cast<unsigned char>(b[i])))
      return false;
  }
  return true;
}

sol::protected_function Resolve(sol::state& L, const char* name) {
  sol::object o = L[name];
  return o.is<sol::protected_function>() ? o.as<sol::protected_function>() : sol::protected_function();
}

template <typename... Args>
void DispatchVoid(sol::protected_function& fn, const char* name, Args&&... args) {
  if (!g.active || !fn.valid()) return;
  Guard::Scope guard;   // a fault in a game call ends this callback as a Lua error
  Budget::Call budget;  // so does running past the instruction limit
  try {
    sol::protected_function_result r = fn(std::forward<Args>(args)...);
    if (!r.valid()) { sol::error e = r; LogCbError(name, e.what()); }
  } catch (const std::exception& e) { LogCbError(name, e.what()); }
    catch (...) { LogCbError(name, "unknown C++ exception"); }
}

template <typename... Args>
bool DispatchBool(sol::protected_function& fn, const char* name, bool dflt, Args&&... args) {
  if (!g.active || !fn.valid()) return dflt;
  Guard::Scope guard;
  Budget::Call budget;
  try {
    sol::protected_function_result r = fn(std::forward<Args>(args)...);
    if (!r.valid()) { sol::error e = r; LogCbError(name, e.what()); return dflt; }
    if (r.return_count() < 1) return dflt;
    sol::object ret = r.get<sol::object>(0);
    if (!ret.valid() || ret == sol::lua_nil) return false;
    if (ret.is<bool>()) return ret.as<bool>();
    return true;
  } catch (const std::exception& e) { LogCbError(name, e.what()); return dflt; }
    catch (...) { LogCbError(name, "unknown C++ exception"); return dflt; }
}

// One-time core initialization (master log + crash reporter + pin). Called on first mission load.
void EnsureCoreInit() {
  static bool inited = false;
  if (inited) return;
  inited = true;
  std::error_code ec;
  fs::create_directories("logs", ec);          // cwd is Outpost2\OPU at runtime
  InitMainLog("logs/OP2Lua.log", false);       // the persistent framework record
  ErrorHandling::Enable();
  SetMarkProvider(&CurrentMarkForLog);          // stamp every log line with the game mark

  // Pin this DLL for the process lifetime. When OP2 ends a mission it frees the stub DLL, which
  // would otherwise drop this shared core's refcount to zero and unload it mid-game (crashing in
  // FreeModuleDesc). Pinning keeps it resident across missions.
  HMODULE pinned = nullptr;
  GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                     reinterpret_cast<LPCWSTR>(&EnsureCoreInit), &pinned);
}

// Log the running core's version + when the DLL was built - the ground truth for "which build is this".
// Logged once per mission load.
void LogBuildStamp() {
  LOG(INFO) << "OP2LuaCore " OP2LUA_VERSION " (DLL built " << BuildTime() << ")";
}

// Log which .map the mission actually loads. OP2 reads the map filename from the stub DLL's exported
// MapName[] *before* any Lua runs, so THIS - not placement.lua's `map=` field, which is author
// metadata only and never drives loading - is the ground truth. Logging it makes a stale bake
// obvious (e.g. placement.lua says eden01.map but the DLL still forces newworld.map: re-patch the
// DLL to change the map). `base` is the stub's own filename, so <base>.dll is loaded in-process.
void LogMissionMap(const std::string& base) {
  if (base.empty()) return;
  const std::string dll = base + ".dll";
  const HMODULE h = GetModuleHandleA(dll.c_str());
  if (h == nullptr) {
    LOG(INFO) << "Mission map: <could not locate stub '" << dll << "' in process>";
    return;
  }
  const auto* mapName   = reinterpret_cast<const char*>(GetProcAddress(h, "MapName"));
  const auto* levelDesc = reinterpret_cast<const char*>(GetProcAddress(h, "LevelDesc"));
  const std::string baked = (mapName != nullptr) ? mapName : "";
  LOG(INFO) << "Mission map: " << (baked.empty() ? "<MapName export missing>" : baked)
            << " (baked into " << dll << "; LevelDesc \"" << (levelDesc ? levelDesc : "?") << "\")";

  // Cross-check placement.lua's `map=` field. That field is author metadata only and never drives
  // loading (OP2 reads MapName from the DLL above), so if the two disagree the DLL silently wins -
  // warn loudly, because editing only placement.lua looks like it should change the map but doesn't.
  const std::string declared = Placement::DeclaredMap();
  if (!declared.empty() && !baked.empty() && !EqualsIgnoreCase(declared, baked)) {
    LOG(WARNING) << "Map mismatch: placement.lua declares \"" << declared << "\" but " << dll
                 << " bakes \"" << baked << "\". The DLL wins - re-patch the DLL to change the map.";
  }
}

// Filename-safe form of the stub base name (keep alnum / '-' / '_').
std::string CleanBase(const std::string& base) {
  std::string clean;
  for (char c : base) {
    if (std::isalnum(static_cast<unsigned char>(c)) || c == '-' || c == '_') clean += c;
  }
  return clean;
}

// Per-mission log file names: logs/OP2Lua-<base>.log (events) and logs/OP2Lua-<base>-AI.log (AI debug),
// so each mission DLL gets its own logs. Empty base -> no per-mission file (everything goes to Main).
std::string MissionLogPath(const std::string& base) {
  const std::string clean = CleanBase(base);
  return clean.empty() ? std::string() : ("logs/OP2Lua-" + clean + ".log");
}
std::string AILogPath(const std::string& base) {
  const std::string clean = CleanBase(base);
  return clean.empty() ? std::string() : ("logs/OP2Lua-" + clean + "-AI.log");
}
std::string AIStatusPath(const std::string& base) {
  const std::string clean = CleanBase(base);
  return clean.empty() ? std::string() : ("logs/OP2Lua-" + clean + "-AI-status.json");
}
std::string ResultPath(const std::string& base) {
  const std::string clean = CleanBase(base);
  return clean.empty() ? std::string() : ("logs/OP2Lua-" + clean + "-result.json");
}

// The game's GameTermReasons as a word: single player won/lost/aborted, multiplayer victory/defeat/terminated.
const char* ResultName(int reason) {
  switch (reason) {
    case 3: return "won";
    case 4: return "lost";
    case 6: return "aborted";
    case 7: return "victory";
    case 8: return "defeat";
    case 9: return "terminated";
    default: return "other";
  }
}

// The mission's end, recorded once, from whichever comes first: the game's own end reason as a tick sees it, the
// MissionResults OPU passes OnEndMission, or the end reason when the game unloads the mission. Written to
// OPU\logs\OP2Lua-<dll>-result.json (overwritten each end, so a launcher can read how the mission ended) and given to
// on_end as a table.
struct EndResult { int reason = 0, tick = 0, localPlayer = 0; };
bool gEnded = false;

void FinishMission(const EndResult& e, const char* source, bool callLua = true) {
  if (gEnded) return;
  gEnded = true;
  const char* result = ResultName(e.reason);
  const std::string path = ResultPath(gBase);
  if (!path.empty()) {
    char when[32] = "";
    const std::time_t now = std::time(nullptr);
    std::tm local{};
    if (localtime_s(&local, &now) == 0) std::strftime(when, sizeof(when), "%Y-%m-%dT%H:%M:%S", &local);
    if (FILE* f = std::fopen(path.c_str(), "w")) {
      std::fprintf(f,
                   "{\n  \"mission\": \"%s.dll\",\n  \"result\": \"%s\",\n  \"reason\": %d,\n  \"tick\": %d,\n"
                   "  \"mark\": %d,\n  \"local_player\": %d,\n  \"time\": \"%s\"\n}\n",
                   CleanBase(gBase).c_str(), result, e.reason, e.tick, e.tick / 100, e.localPlayer, when);
      std::fclose(f);
    } else {
      LOG(WARNING) << "could not write " << path;
    }
  }
  LOG_MISSION(INFO) << "mission ended: " << result << " (reason " << e.reason << ", tick " << e.tick << ", seen by "
                    << source << ")";
  if (!callLua || !g.active || !g.onEnd.valid()) return;
  sol::table t = LuaHost::Get().lua().create_table();
  t["result"] = result;
  t["reason"] = e.reason;
  t["tick"] = e.tick;
  t["mark"] = e.tick / 100;
  t["local_player"] = e.localPlayer;
  DispatchVoid(g.onEnd, "on_end", t);
}

// The game's end reason now (GameImpl::gameTermReasons_); 0 while the game runs.
EndResult GameEnd() {
  EndResult e;
  if (auto* impl = Tethys::GameImpl::GetInstance()) {
    e.reason = static_cast<int>(impl->gameTermReasons_);
    e.tick = impl->tick_;
    e.localPlayer = impl->localPlayer_;
  }
  return e;
}

// When the game exits without telling the mission it ended (a game started by op2launcher closes from the end
// screen: no OnEndMission, no OnUnloadMission), the core's shutdown records the end. No Lua that late.
void FinishAtExit() {
  if (!gEnded && !gBase.empty()) FinishMission(GameEnd(), "the game's exit", false);
}

// Logs a mission event the game sent, with the end reason the game had then.
void NoteEndEvent(const char* name) {
  const EndResult e = GameEnd();
  LOG_MISSION(INFO) << name << " (end reason " << e.reason << ", tick " << e.tick << ")";
}

// The .map name baked into the stub DLL's MapName export (the ground truth - what OP2 actually loads).
std::string BakedMapName(const std::string& base) {
  if (base.empty()) return "";
  const HMODULE h = GetModuleHandleA((base + ".dll").c_str());
  if (h == nullptr) return "";
  const auto* mapName = reinterpret_cast<const char*>(GetProcAddress(h, "MapName"));
  return (mapName != nullptr) ? mapName : "";
}

// Returns `preferred` if it exists on disk, else `fallback`.
std::string PickExisting(const std::string& preferred, const std::string& fallback) {
  std::error_code ec;
  return fs::exists(preferred, ec) ? preferred : fallback;
}

// First of the candidates that exists on disk, else the last (the generic fallback).
std::string PickExisting(const std::string& a, const std::string& b, const std::string& fallback) {
  std::error_code ec;
  if (fs::exists(a, ec)) return a;
  if (fs::exists(b, ec)) return b;
  return fallback;
}

// OP2 lists a mission under "colony games" via a leading 'c' on the DLL filename - but the .lua scripts
// don't need that prefix. So for a stub named e.g. "cStrikeTeam" we also look for the unprefixed
// "StrikeTeam.{lua,placement.lua}". Only strips a 'c' followed by an uppercase letter (the CamelCase
// colony-naming convention), so an actual name like "convoy" is left alone.
std::string StripColonyPrefix(const std::string& base) {
  if ((base.size() >= 2) && ((base[0] == 'c') || (base[0] == 'C')) && (base[1] >= 'A') && (base[1] <= 'Z'))
    return base.substr(1);
  return base;
}

// Load a mission's scripts into a fresh Lua state and resolve its callbacks.
// Prefers <dir>/<base>.lua (+ <base>.placement.lua); falls back to mission.lua (+ placement.lua).
void LoadMissionDir(const std::string& dir, const std::string& base) {
  EnsureCoreInit();                      // one-time: main log + crash reporter + mark stamp + pin
  gDir = dir;
  gBase = base;
  gEnded = false;
  static const bool atExitSet = (std::atexit(FinishAtExit), true);   // after the crash reporter's: runs before it
  (void)atExitSet;

  // Point the per-mission Mission + AI logs at this mission, with a self-identifying header on each
  // (name + baked map + core version) so neither file needs the framework lines duplicated into it.
  const std::string ver   = "OP2LuaCore " OP2LUA_VERSION;
  const std::string baked = BakedMapName(base);
  const std::string label = base.empty() ? std::string("mission") : base;
  std::string missionHeader = label + (baked.empty() ? "" : ("  map " + baked)) + "  (" + ver + ")";
  SetMissionLog(MissionLogPath(base), missionHeader);
  SetAILog(AILogPath(base), label + " scripted AI  (" + ver + ")");
  SetAIStatusPath(AIStatusPath(base));   // log.ai_status overwrites this snapshot file each call
  LogBuildStamp();                       // stamp version + DLL build time (-> Main log)
  Reset();                               // release any previous callbacks/placement (old state alive)
  LuaHost::Get().Init();      // fresh Lua state + bindings for this mission

  const std::string alt = StripColonyPrefix(base);   // cStrikeTeam -> StrikeTeam (scripts need no 'c')
  Placement::Load(PickExisting(dir + "\\" + base + ".placement.lua",
                               dir + "\\" + alt  + ".placement.lua",
                               dir + "\\placement.lua"));
  LogMissionMap(base);   // baked .map (ground truth) + warn if placement.lua's map= disagrees

  const std::string missionLua = PickExisting(dir + "\\" + base + ".lua",
                                              dir + "\\" + alt  + ".lua",
                                              dir + "\\mission.lua");
  if (!LuaHost::Get().RunFile(missionLua)) return;  // RunFile reports load errors

  sol::state& L = LuaHost::Get().lua();
  g.onInit        = Resolve(L, "on_init");
  g.onTick        = Resolve(L, "on_tick");
  g.onChat        = Resolve(L, "on_chat");
  g.onCreateUnit  = Resolve(L, "on_create_unit");
  g.onDestroyUnit = Resolve(L, "on_destroy_unit");
  g.onDamageUnit  = Resolve(L, "on_damage_unit");
  g.onLoad        = Resolve(L, "on_load");
  g.onUnload      = Resolve(L, "on_unload");
  g.onEnd         = Resolve(L, "on_end");
  g.onSave        = Resolve(L, "on_save");
  g.onLoadSave    = Resolve(L, "on_load_save");
  g.pump          = Resolve(L, "__op2lua_pump");  // scheduler pump from the preamble
  g.active = true;
  LOG(INFO) << "Mission loaded: " << missionLua;
}

}  // namespace

std::string Dir() { return gDir; }

std::string BuildTime() {
  HMODULE hSelf = nullptr;
  if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                          reinterpret_cast<LPCWSTR>(&BuildTime), &hSelf)) {
    return "unknown";
  }
  // The link time in the loaded image's PE header (the COFF TimeDateStamp, seconds since 1970 UTC): unlike the
  // file's modified time it stays the same when the DLL is copied, and it's what OP2LuaEditor shows.
  const auto* base = reinterpret_cast<const unsigned char*>(hSelf);
  const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
  const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
  const __time64_t stamp = nt->FileHeader.TimeDateStamp;
  tm local = {};
  if (_localtime64_s(&local, &stamp) != 0) return "unknown";
  char text[32] = {};
  std::strftime(text, sizeof(text), "%Y-%m-%d %H:%M:%S", &local);
  return text;
}

std::string GameVersion() {
  // TApp::Version, called on the game's TApp: the version the running game goes by, the one its lobby compares with a
  // joining player's ("wrong game version"). 0x01020007 is 1.2.0.7 (the GOG exe alone); OPU 1.4.x makes it 1.4.0.1.
  // Both are Outpost2.exe exports, found by name. TApp::Version is __thiscall with no arguments: a __fastcall
  // pointer passes `this` in ECX the same way (EDX is ignored).
  HMODULE exe = GetModuleHandleW(nullptr);
  using VersionFn = unsigned long(__fastcall*)(void* self, void* unused);
  const auto version = reinterpret_cast<VersionFn>(GetProcAddress(exe, "?Version@TApp@@QAEKXZ"));
  void* const app = reinterpret_cast<void*>(GetProcAddress(exe, "?gTApp@@3VTApp@@A"));
  if (version == nullptr || app == nullptr) return std::string();
  const unsigned long v = version(app, nullptr);
  char text[32] = {};
  std::snprintf(text, sizeof(text), "%lu.%lu.%lu.%lu", (v >> 24) & 0xFF, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
  return text;
}

std::string GameFileVersion() {
  // The FileVersion of the running Outpost2.exe, from the version resource in its loaded image (resource 1 of type
  // RT_VERSION). The VS_FIXEDFILEINFO block sits a few bytes in, after the resource's header and key, on a 4-byte
  // boundary; it starts with the signature 0xFEEF04BD.
  HMODULE exe = GetModuleHandleW(nullptr);
  HRSRC res = FindResourceW(exe, MAKEINTRESOURCEW(1), MAKEINTRESOURCEW(16));   // RT_VERSION
  if (res == nullptr) return std::string();
  const auto* data = static_cast<const unsigned char*>(LockResource(LoadResource(exe, res)));
  const DWORD size = SizeofResource(exe, res);
  if (data == nullptr) return std::string();
  for (DWORD i = 0; i + sizeof(VS_FIXEDFILEINFO) <= size; i += 4) {
    const auto* ffi = reinterpret_cast<const VS_FIXEDFILEINFO*>(data + i);
    if (ffi->dwSignature != 0xFEEF04BD) continue;
    char text[32] = {};
    std::snprintf(text, sizeof(text), "%u.%u.%u.%u", HIWORD(ffi->dwFileVersionMS), LOWORD(ffi->dwFileVersionMS),
                  HIWORD(ffi->dwFileVersionLS), LOWORD(ffi->dwFileVersionLS));
    return text;
  }
  return std::string();
}

void Reset() {
  g = MissionState();
  Placement::Reset();
  OP2Lua::ResetUnitOrders();
  OP2Lua::StopMissionWav();
  OP2Lua::ResetTriggers();
  OP2Lua::Hackaray::Reset();
}

}  // namespace OP2Lua::Mission


// ---- Exported C API (OP2LuaCore.h) -----------------------------------------------------------
using namespace OP2Lua::Mission;

extern "C" {

OP2LUACORE_API void OP2LuaCore_LoadMission(const char* missionDir, const char* baseName) {
  LoadMissionDir(missionDir != nullptr ? missionDir : "", baseName != nullptr ? baseName : "");
}

OP2LUACORE_API int OP2LuaCore_InitProc(void) {
  {
    OP2Lua::Guard::Scope guard;
    try {
      OP2Lua::Placement::Apply();  // build the level before on_init runs
    } catch (const std::exception& e) {
      LOG_MISSION(ERROR) << "placement: " << e.what() << " - the rest of the layout wasn't placed";
    }
  }
  return DispatchBool(g.onInit, "on_init", true) ? 1 : 0;
}

OP2LUACORE_API void OP2LuaCore_AIProc(void) {
  {
    OP2Lua::Guard::Scope guard;
    try {
      OP2Lua::FlushUnitOrders();      // issue orders queued last tick (units now safe to command)
    } catch (const std::exception& e) {
      LOG_MISSION(ERROR) << "queued unit orders: " << e.what();
    }
  }
  DispatchVoid(g.pump, "scheduler");  // fire due after()/every()/when() (may queue new orders)
  DispatchVoid(g.onTick, "on_tick");
  if (!gEnded) {
    const EndResult e = GameEnd();
    if (e.reason != 0) FinishMission(e, "the game's end reason");
  }
}

OP2LUACORE_API void OP2LuaCore_GetSaveRegions(void* pSaveRegion) {
  auto* s = static_cast<SaveRegion*>(pSaveRegion);
  if (s != nullptr) { s->pData = nullptr; s->size = 0; }
}

OP2LUACORE_API int  OP2LuaCore_OnLoadMission(void*)     { return DispatchBool(g.onLoad, "on_load", true) ? 1 : 0; }
OP2LUACORE_API void OP2LuaCore_OnEndMission(void* pArgs) {
  NoteEndEvent("OnEndMission");
  EndResult e = GameEnd();
  const auto* a = static_cast<const OnEndMissionArgs*>(pArgs);
  if (a != nullptr && a->structSize >= sizeof(OnEndMissionArgs) && a->pMissionResults != nullptr) {
    e.reason = static_cast<int>(a->pMissionResults->gameTermReason);
    e.tick = a->pMissionResults->tick;
    e.localPlayer = a->pMissionResults->localPlayerID;
  }
  FinishMission(e, "OnEndMission");
}
OP2LUACORE_API int  OP2LuaCore_OnSaveGame(void*)        { return DispatchBool(g.onSave, "on_save", true) ? 1 : 0; }
OP2LUACORE_API int  OP2LuaCore_OnLoadSavedGame(void*)   { return DispatchBool(g.onLoadSave, "on_load_save", true) ? 1 : 0; }

OP2LUACORE_API int OP2LuaCore_OnUnloadMission(void*) {
  NoteEndEvent("OnUnloadMission");
  if (!gEnded && !gBase.empty()) FinishMission(GameEnd(), "OnUnloadMission");
  const int r = DispatchBool(g.onUnload, "on_unload", true) ? 1 : 0;
  Reset();  // tear the mission down when the game unloads it
  return r;
}

OP2LUACORE_API void OP2LuaCore_OnChat(void* pArgs) {
  OP2Lua::ChatArgs chat{ static_cast<OnChatArgs*>(pArgs) };
  DispatchVoid(g.onChat, "on_chat", &chat);
}

OP2LUACORE_API void OP2LuaCore_OnCreateUnit(void* pArgs) {
  auto* a = static_cast<OnCreateUnitArgs*>(pArgs);
  if (a == nullptr) return;
  DispatchVoid(g.onCreateUnit, "on_create_unit", a->unit);
}

OP2LUACORE_API void OP2LuaCore_OnDestroyUnit(void* pArgs) {
  auto* a = static_cast<OnDestroyUnitArgs*>(pArgs);
  if (a != nullptr) DispatchVoid(g.onDestroyUnit, "on_destroy_unit", a->unit);
}

OP2LUACORE_API void OP2LuaCore_OnDamageUnit(void* pArgs) {
  auto* a = static_cast<OnDamageUnitArgs*>(pArgs);
  if (a != nullptr) DispatchVoid(g.onDamageUnit, "on_damage_unit", a->targetUnit, a->sourceUnit, a->damage);
}

OP2LUACORE_API void OP2LuaCore_OnTrigger(void* pArgs) {
  if (g.active) OP2Lua::FireTrigger(pArgs);   // the `on=` function of the trigger that fired
}

}  // extern "C"
