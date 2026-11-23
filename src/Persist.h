#pragma once

// Persist - a Lua state's values as bytes, for saved games (SaveGame.cpp).
//
// What a save holds: the values reachable from a root table (the globals, the triggers' functions) - tables, strings,
// numbers, the script's functions with their upvalues, and the game's handles (Unit, Trigger, Group, Player, Region,
// UnitBlock). A function is saved as its prototype's place in its chunk (the preamble, placement or mission script),
// never as code: a save resumes only with the same script, which each chunk's hash checks. Upvalues two functions
// share stay shared. The libraries' and bindings' own values (string.format, game.tick, the game table itself) are
// "permanents": saved by their path from the globals or the registry, and the tables among them as the changes the
// script made to them.

#include <cstdint>
#include <string>
#include <string_view>

struct lua_State;

namespace OP2Lua::Persist {

class Writer {
 public:
  std::string out;
  void U8(uint8_t v) { out.push_back(static_cast<char>(v)); }
  void U32(uint32_t v);
  void I32(int32_t v) { U32(static_cast<uint32_t>(v)); }
  void I64(int64_t v);
  void F64(double v);
  void Str(std::string_view s);
  void Bytes(const void* p, size_t n) { out.append(static_cast<const char*>(p), n); }
};

class Reader {
 public:
  Reader(const void* data, size_t size) : p_(static_cast<const uint8_t*>(data)), n_(size) {}
  uint8_t U8();
  uint32_t U32();
  int32_t I32() { return static_cast<int32_t>(U32()); }
  int64_t I64();
  double F64();
  std::string Str();
  bool Bytes(void* dst, size_t n);
  bool Ok() const { return ok_; }
  void Fail() { ok_ = false; }
  size_t Left() const { return n_ - at_; }

 private:
  const uint8_t* p_;
  size_t n_, at_ = 0;
  bool ok_ = true;
};

// A fresh Lua state: forget the previous state's chunks and permanents.
void Reset();

// Records the values the fresh state starts with (libraries and bindings) as permanents. LuaHost::Init calls it after
// the bindings and before the preamble runs.
void SnapshotPermanents(lua_State* L);

// Registers the compiled chunk at the top of the stack (left there) as `name`, compiled from `source`: its functions'
// prototypes get their places, and the chunk stays alive with the state.
void AddChunk(lua_State* L, const std::string& name, std::string_view source);

// Writes the value at stack index `idx` (with the chunks its functions come from, and the changes the script made to
// the permanent tables it doesn't reach). False with `err`, naming where the value was, for one that can't be saved.
bool Save(lua_State* L, int idx, Writer& w, std::string& err);
// Reads a value written by Save and pushes it. False with `err` (nothing pushed) for a broken save, or one made with
// another version of a chunk it needs.
bool Restore(lua_State* L, Reader& r, std::string& err);

// FNV-1a, 64 bits, but from the starting value 1469598103934665603, not FNV's usual 14695981039346656037. Saves hold
// these hashes and tools read them (FILE-FORMAT.md, The state's header), so it stays as it is.
uint64_t Hash(std::string_view s);

}  // namespace OP2Lua::Persist
