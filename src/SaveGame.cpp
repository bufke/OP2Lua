// SaveGame.cpp - a Lua mission's state in the game's saved games. See SaveGame.h.

#include "SaveGame.h"

#include <cstdint>
#include <cstring>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include "Log.h"
#include "Mission.h"

#include "Tethys/API/Mission.h"          // SaveRegion
#include "Tethys/Resource/StreamIO.h"    // StreamIO (Tell/Seek)

using Tethys::StreamIO;
using Tethys::TethysAPI::SaveRegion;

namespace OP2Lua::SaveGame {
namespace {

constexpr uintptr_t kSaveCall      = 0x48A2D7;  // TethysGame's save: call MissionManager::Save (0x402C60)
constexpr uintptr_t kMissionSave   = 0x402C60;
constexpr uintptr_t kSizeCheck     = 0x402E9A;  // MissionManager::Load: mov edx,[esi+0x14]; cmp edx,eax; je; fail
constexpr uintptr_t kStubsLoadCall = 0x402ED6;  // MissionManager::Load: call ScStubList::Load (0x47ACE0)
constexpr uintptr_t kStubsLoad     = 0x47ACE0;
constexpr uintptr_t kMissionManager = 0x4EFD18;
constexpr uint32_t  kMaxRegion     = 64u << 20;   // more than any script keeps; a bigger size is a broken save

// The region a Lua mission gives the game: this buffer. The game reads a save's region into it.
std::vector<char> gRegion;
void* gRegionPtr = nullptr;   // what the region's pData was set to (whether the loaded DLL's region is ours)
bool gNoRegionInSave = false;

SaveRegion* MissionRegion() { return reinterpret_cast<SaveRegion*>(kMissionManager + 0x10); }
bool RegionIsOurs(const SaveRegion* r) { return gRegionPtr != nullptr && r->pData == gRegionPtr; }

void PointRegionAtBuffer(SaveRegion* r, size_t size) {
  if (gRegion.empty()) gRegion.resize(1);
  r->pData = gRegion.data();
  r->size = size;
  gRegionPtr = r->pData;
}

// ---- the hooks ----------------------------------------------------------------------------------------------------
using MissionSaveFn = int(__thiscall*)(void* self, StreamIO* s);
using StubsLoadFn = int(__thiscall*)(void* self, StreamIO* s);

// MissionManager::Save, as TethysGame's save calls it (thiscall: __fastcall takes `this` in ECX the same way).
int __fastcall SaveHook(void* self, void*, StreamIO* s) {
  SaveRegion* r = MissionRegion();
  if (RegionIsOurs(r)) {
    std::vector<char> state;
    std::string err;
    if (!Mission::BuildSave(state, err)) {
      LOG_MISSION(ERROR) << "saved game: the game was not saved: " << err;
      return 0;   // the game's save fails (and says so); the slot's file is already part written
    }
    gRegion = std::move(state);
    PointRegionAtBuffer(r, gRegion.size());
  }
  return reinterpret_cast<MissionSaveFn>(kMissionSave)(self, s);
}

// MissionManager::Load's region size check: eax is the size in the save, [esi+0x10] the region, edi the stream, and
// MissionManager::Load's local at [esp+0xC] the size it reads next. Returns 1 to read the region, 0 to fail the load.
extern "C" int __cdecl AcceptRegionSize(SaveRegion* r, uint32_t size, StreamIO* s, uint32_t* pSize) {
  if (!RegionIsOurs(r)) return size == r->size ? 1 : 0;   // another mission's region: the game's own check
  gNoRegionInSave = false;
  if (size == 0xFFFFFFFFu) {
    // No region in this save: the word is the first default fight group's stub index, -1 for the human player 0 (the
    // block after the region, MissionManager +0x1C). Step back over it and read no region.
    const size_t at = s->Tell();
    if (at < 4 || !s->Seek(at - 4)) return 0;
    *pSize = 0;
    gNoRegionInSave = true;
    return 1;
  }
  if (size > kMaxRegion) return 0;
  gRegion.assign(size > 0 ? size : 1, 0);
  PointRegionAtBuffer(r, size);
  return 1;
}

__declspec(naked) void SizeCheckThunk() {
  __asm {
    push ecx                 // MissionManager::Load reads the region with ecx (the size) after this
    push edx
    lea edx, [esp + 0x18]    // its [esp+0xC]: past the return address and the two pushes
    push edx
    push edi
    push eax
    lea edx, [esi + 0x10]
    push edx
    call AcceptRegionSize
    add esp, 16
    pop edx
    pop ecx
    ret
  }
}

// ScStubList::Load inside MissionManager::Load: the region has been read and the game's triggers and groups loaded.
int __fastcall StubsLoadHook(void* self, void*, StreamIO* s) {
  const int ok = reinterpret_cast<StubsLoadFn>(kStubsLoad)(self, s);
  if (ok && RegionIsOurs(MissionRegion())) {
    if (gNoRegionInSave) Mission::LoadedWithoutState();
    else Mission::RestoreSave(gRegion.data(), MissionRegion()->size);
  }
  return ok;
}

// ---- patching -----------------------------------------------------------------------------------------------------
bool Matches(uintptr_t at, const uint8_t* bytes, size_t n) { return std::memcmp(reinterpret_cast<void*>(at), bytes, n) == 0; }

void Write(uintptr_t at, const uint8_t* bytes, size_t n) {
  DWORD old = 0;
  VirtualProtect(reinterpret_cast<void*>(at), n, PAGE_EXECUTE_READWRITE, &old);
  std::memcpy(reinterpret_cast<void*>(at), bytes, n);
  VirtualProtect(reinterpret_cast<void*>(at), n, old, &old);
  FlushInstructionCache(GetCurrentProcess(), reinterpret_cast<void*>(at), n);
}

void CallBytes(uint8_t out[5], uintptr_t at, uintptr_t target) {
  out[0] = 0xE8;
  const int32_t rel = static_cast<int32_t>(target - (at + 5));
  std::memcpy(out + 1, &rel, 4);
}

}  // namespace

bool Install() {
  static int state = 0;   // 0 not tried, 1 installed, -1 refused
  if (state != 0) return state > 0;
  uint8_t saveCall[5], stubsCall[5];
  CallBytes(saveCall, kSaveCall, kMissionSave);
  CallBytes(stubsCall, kStubsLoadCall, kStubsLoad);
  static const uint8_t kSizeCheckBytes[] = { 0x8B, 0x56, 0x14, 0x3B, 0xD0, 0x74, 0x04, 0x33, 0xC0, 0xEB, 0x64 };
  if (!Matches(kSaveCall, saveCall, 5) || !Matches(kSizeCheck, kSizeCheckBytes, sizeof kSizeCheckBytes) ||
      !Matches(kStubsLoadCall, stubsCall, 5)) {
    state = -1;
    LOG(WARNING) << "saved games: Outpost2.exe isn't the 1.2.0.7 the core knows (or another module changed its save "
                    "code); a saved game of a Lua mission won't keep the script's state";
    return false;
  }
  uint8_t bytes[11];
  CallBytes(bytes, kSaveCall, reinterpret_cast<uintptr_t>(&SaveHook));
  Write(kSaveCall, bytes, 5);
  CallBytes(bytes, kStubsLoadCall, reinterpret_cast<uintptr_t>(&StubsLoadHook));
  Write(kStubsLoadCall, bytes, 5);
  // call SizeCheckThunk; test eax,eax; jnz +2 (go on: read the region); jmp 0x402F09 (fail, eax 0)
  CallBytes(bytes, kSizeCheck, reinterpret_cast<uintptr_t>(&SizeCheckThunk));
  const uint8_t rest[] = { 0x85, 0xC0, 0x75, 0x02, 0xEB, 0x64 };
  std::memcpy(bytes + 5, rest, sizeof rest);
  Write(kSizeCheck, bytes, 11);
  state = 1;
  LOG(INFO) << "saved games: hooks installed (a Lua mission's script state goes in the save)";
  return true;
}

void UseRegion(void* pSaveRegion) {
  auto* r = static_cast<SaveRegion*>(pSaveRegion);
  if (r == nullptr) return;
  if (!Install()) { r->pData = nullptr; r->size = 0; gRegionPtr = nullptr; return; }
  gNoRegionInSave = false;
  gRegion.assign(1, 0);
  PointRegionAtBuffer(r, 0);
}

}  // namespace OP2Lua::SaveGame
