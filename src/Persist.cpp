// Persist.cpp - a Lua state's values as bytes, for saved games. See Persist.h.

#include "Persist.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <unordered_map>
#include <utility>
#include <vector>

#include "lua.hpp"
#include "persist_lua.h"

namespace OP2Lua {
namespace Persist { class Writer; class Reader; }
// Bindings.cpp: a game handle (a Unit, Trigger, Group, Player, Region or UnitBlock userdata) as its kind and fields,
// and the handle made again from them (pushed). False for a userdata that isn't one of them, or a broken record.
bool WriteHandle(lua_State* L, int idx, Persist::Writer& w);
bool PushHandle(lua_State* L, uint8_t kind, Persist::Reader& r);
}  // namespace OP2Lua

namespace OP2Lua::Persist {

// ---- bytes --------------------------------------------------------------------------------------------------------

void Writer::U32(uint32_t v) { for (int i = 0; i < 4; ++i) U8(static_cast<uint8_t>(v >> (8 * i))); }
void Writer::I64(int64_t v) {
  const uint64_t u = static_cast<uint64_t>(v);
  U32(static_cast<uint32_t>(u));
  U32(static_cast<uint32_t>(u >> 32));
}
void Writer::F64(double v) {
  int64_t bits = 0;
  std::memcpy(&bits, &v, sizeof bits);
  I64(bits);
}
void Writer::Str(std::string_view s) {
  U32(static_cast<uint32_t>(s.size()));
  out.append(s.data(), s.size());
}

uint8_t Reader::U8() {
  if (at_ >= n_) { ok_ = false; return 0; }
  return p_[at_++];
}
uint32_t Reader::U32() {
  uint32_t v = 0;
  for (int i = 0; i < 4; ++i) v |= static_cast<uint32_t>(U8()) << (8 * i);
  return v;
}
int64_t Reader::I64() {
  const uint64_t lo = U32(), hi = U32();
  return static_cast<int64_t>(lo | (hi << 32));
}
double Reader::F64() {
  const int64_t bits = I64();
  double v = 0;
  std::memcpy(&v, &bits, sizeof v);
  return v;
}
std::string Reader::Str() {
  const uint32_t len = U32();
  if (!ok_ || len > Left()) { ok_ = false; return {}; }
  std::string s(reinterpret_cast<const char*>(p_ + at_), len);
  at_ += len;
  return s;
}
bool Reader::Bytes(void* dst, size_t n) {
  if (!ok_ || n > Left()) { ok_ = false; return false; }
  std::memcpy(dst, p_ + at_, n);
  at_ += n;
  return true;
}

uint64_t Hash(std::string_view s) {
  uint64_t h = 1469598103934665603ull;
  for (unsigned char c : s) { h ^= c; h *= 1099511628211ull; }
  return h;
}

namespace {

// ---- value tags ---------------------------------------------------------------------------------------------------
enum Tag : uint8_t {
  kNil = 1, kFalse, kTrue, kInt, kNum, kStr,
  kRef,         // an object already read: its id
  kTable,       // id, then key/value pairs to kEnd, then the metatable
  kPerm,        // id, a permanent's path
  kPermTable,   // id, a permanent table's path, the pairs that differ from its start to kEnd, the keys it lost to kEnd,
                // then its metatable
  kFunc,        // id, chunk, prototype, upvalue count, then each upvalue (kUpNew id value | kUpRef id)
  kHandle,      // id, a game handle (Bindings.cpp's kind and fields)
  kEnd,
  kUpNew, kUpRef,
};

constexpr int kMaxDepth = 200;   // nesting the save follows before it gives up (the C stack)

// Registry keys (their addresses) of the state's permanents.
char kPermByObj, kPermByPath, kSnapshots, kChunkFns;

// The chunks the state compiled, and where each prototype is.
struct Chunk {
  std::string name;
  uint64_t hash = 0;
  std::vector<const void*> protos;
};
std::vector<Chunk> gChunks;
std::unordered_map<const void*, std::pair<uint32_t, uint32_t>> gPlace;   // prototype -> chunk, index

// A key as part of a permanent's path: type letter and text. Only strings, integers and booleans make paths.
bool KeySegment(lua_State* L, int idx, std::string& out) {
  switch (lua_type(L, idx)) {
    case LUA_TSTRING: { size_t n = 0; const char* s = lua_tolstring(L, idx, &n); out.assign("s").append(s, n); return true; }
    case LUA_TNUMBER:
      if (!lua_isinteger(L, idx)) return false;
      out = "i" + std::to_string(static_cast<long long>(lua_tointeger(L, idx)));
      return true;
    case LUA_TBOOLEAN: out = lua_toboolean(L, idx) ? "b1" : "b0"; return true;
    default: return false;
  }
}

// A key for an error message.
std::string KeyText(lua_State* L, int idx) {
  switch (lua_type(L, idx)) {
    case LUA_TSTRING: return lua_tostring(L, idx);
    case LUA_TNUMBER: {
      lua_pushvalue(L, idx);
      std::string s = lua_tostring(L, -1);
      lua_pop(L, 1);
      return "[" + s + "]";
    }
    default: return std::string("[") + luaL_typename(L, idx) + "]";
  }
}

// A path as a person reads it: the segments joined by dots.
std::string PathText(const std::string& path) {
  std::string out;
  size_t start = 0;
  while (start <= path.size()) {
    size_t end = path.find('\x1f', start);
    if (end == std::string::npos) end = path.size();
    const std::string seg = path.substr(start, end - start);
    if (!out.empty()) out += '.';
    out += seg.size() > 1 ? seg.substr(1) : seg;
    start = end + 1;
  }
  return out;
}

void PushRegTable(lua_State* L, const void* key) {
  if (lua_rawgetp(L, LUA_REGISTRYINDEX, key) != LUA_TTABLE) {
    lua_pop(L, 1);
    lua_newtable(L);
    lua_pushvalue(L, -1);
    lua_rawsetp(L, LUA_REGISTRYINDEX, key);
  }
}

bool Collectable(int t) {
  return t == LUA_TTABLE || t == LUA_TFUNCTION || t == LUA_TUSERDATA || t == LUA_TTHREAD || t == LUA_TLIGHTUSERDATA;
}

// ---- save ---------------------------------------------------------------------------------------------------------
struct SaveCtx {
  lua_State* L;
  Writer& w;
  int seen, byObj, snaps;   // stack indices: object -> id, permanents' paths, snapshots
  uint32_t nextId = 1;
  std::unordered_map<const void*, uint32_t> ups;
  uint32_t nextUp = 1;
  std::vector<std::string> trail;   // where the walk is, for an error
  std::vector<bool> chunksUsed;
  std::string err;
  int depth = 0;
};

bool Fail(SaveCtx& c, const std::string& what) {
  std::string where;
  for (const std::string& t : c.trail) where += (where.empty() || t[0] == '[' ? "" : ".") + t;
  c.err = what + (where.empty() ? "" : " (at " + where + ")");
  return false;
}

uint32_t Remember(SaveCtx& c, int idx) {
  lua_pushvalue(c.L, idx);
  lua_pushinteger(c.L, c.nextId);
  lua_rawset(c.L, c.seen);
  return c.nextId++;
}

bool W(SaveCtx& c, int idx);

// The pairs of permanent table `t` that differ from its snapshot `s`, then the keys it no longer has.
bool WriteDiff(SaveCtx& c, int t, int s) {
  lua_State* L = c.L;
  lua_pushnil(L);
  while (lua_next(L, t) != 0) {
    lua_pushvalue(L, -2);
    lua_rawget(L, s);
    const bool same = lua_rawequal(L, -1, -2) != 0;
    lua_pop(L, 1);
    if (!same) {
      c.trail.push_back(KeyText(L, -2));
      if (!W(c, -2) || !W(c, -1)) return false;
      c.trail.pop_back();
    }
    lua_pop(L, 1);
  }
  c.w.U8(kEnd);
  lua_pushnil(L);
  while (lua_next(L, s) != 0) {
    lua_pushvalue(L, -2);
    const bool gone = lua_rawget(L, t) == LUA_TNIL;
    lua_pop(L, 2);
    if (gone && !W(c, -1)) return false;
  }
  c.w.U8(kEnd);
  return true;
}

bool WriteMetatable(SaveCtx& c, int idx) {
  if (!lua_getmetatable(c.L, idx)) { c.w.U8(kNil); return true; }
  c.trail.push_back("<metatable>");
  const bool ok = W(c, -1);
  c.trail.pop_back();
  lua_pop(c.L, 1);
  return ok;
}

bool WriteFunction(SaveCtx& c, int idx) {
  lua_State* L = c.L;
  if (lua_iscfunction(L, idx)) return Fail(c, "a C function that isn't one of OP2Lua's can't be saved");
  const void* proto = op2lua_function_proto(L, idx);
  const auto it = gPlace.find(proto);
  if (it == gPlace.end()) return Fail(c, "a function that isn't from the mission's own files can't be saved");
  lua_Debug ar{};
  lua_pushvalue(L, idx);
  lua_getinfo(L, ">u", &ar);
  const uint32_t id = Remember(c, idx);
  c.w.U8(kFunc);
  c.w.U32(id);
  c.w.U32(it->second.first);
  c.w.U32(it->second.second);
  c.w.U8(ar.nups);
  c.chunksUsed[it->second.first] = true;
  for (int i = 1; i <= ar.nups; ++i) {
    const void* uid = lua_upvalueid(L, idx, i);
    const auto u = c.ups.find(uid);
    if (u != c.ups.end()) { c.w.U8(kUpRef); c.w.U32(u->second); continue; }
    const uint32_t n = c.nextUp++;
    c.ups.emplace(uid, n);
    c.w.U8(kUpNew);
    c.w.U32(n);
    const char* name = lua_getupvalue(L, idx, i);
    c.trail.push_back(std::string("<upvalue ") + (name && *name ? name : "?") + ">");
    const bool ok = W(c, -1);
    c.trail.pop_back();
    lua_pop(L, 1);
    if (!ok) return false;
  }
  return true;
}

bool W(SaveCtx& c, int idx) {
  lua_State* L = c.L;
  idx = lua_absindex(L, idx);
  const int t = lua_type(L, idx);
  switch (t) {
    case LUA_TNIL: c.w.U8(kNil); return true;
    case LUA_TBOOLEAN: c.w.U8(lua_toboolean(L, idx) ? kTrue : kFalse); return true;
    case LUA_TNUMBER:
      if (lua_isinteger(L, idx)) { c.w.U8(kInt); c.w.I64(lua_tointeger(L, idx)); }
      else { c.w.U8(kNum); c.w.F64(lua_tonumber(L, idx)); }
      return true;
    case LUA_TSTRING: {
      size_t n = 0;
      const char* s = lua_tolstring(L, idx, &n);
      c.w.U8(kStr);
      c.w.Str(std::string_view(s, n));
      return true;
    }
    default: break;
  }
  if (++c.depth > kMaxDepth) return Fail(c, "values nested more than 200 deep can't be saved");
  struct Leave { int& d; ~Leave() { --d; } } leave{ c.depth };
  if (!lua_checkstack(L, 10)) return Fail(c, "out of Lua stack");

  lua_pushvalue(L, idx);
  if (lua_rawget(L, c.seen) == LUA_TNUMBER) {
    c.w.U8(kRef);
    c.w.U32(static_cast<uint32_t>(lua_tointeger(L, -1)));
    lua_pop(L, 1);
    return true;
  }
  lua_pop(L, 1);

  lua_pushvalue(L, idx);
  if (lua_rawget(L, c.byObj) == LUA_TSTRING) {
    const std::string path = lua_tostring(L, -1);
    lua_pop(L, 1);
    const uint32_t id = Remember(c, idx);
    lua_pushvalue(L, idx);
    const bool tracked = t == LUA_TTABLE && lua_rawget(L, c.snaps) == LUA_TTABLE;
    if (!tracked) {
      lua_pop(L, 1);
      c.w.U8(kPerm);
      c.w.U32(id);
      c.w.Str(path);
      return true;
    }
    const int snap = lua_gettop(L);
    c.w.U8(kPermTable);
    c.w.U32(id);
    c.w.Str(path);
    std::vector<std::string> outer = std::move(c.trail);   // inside it, errors are named from its own path
    c.trail = { PathText(path) };
    const bool ok = WriteDiff(c, idx, snap) && WriteMetatable(c, idx);
    c.trail = std::move(outer);
    lua_pop(L, 1);
    return ok;
  }
  lua_pop(L, 1);

  switch (t) {
    case LUA_TTABLE: {
      const uint32_t id = Remember(c, idx);
      c.w.U8(kTable);
      c.w.U32(id);
      lua_pushnil(L);
      while (lua_next(L, idx) != 0) {
        c.trail.push_back(KeyText(L, -2));
        if (!W(c, -2) || !W(c, -1)) return false;
        c.trail.pop_back();
        lua_pop(L, 1);
      }
      c.w.U8(kEnd);
      return WriteMetatable(c, idx);
    }
    case LUA_TFUNCTION:
      return WriteFunction(c, idx);
    case LUA_TUSERDATA: {
      const uint32_t id = Remember(c, idx);
      c.w.U8(kHandle);
      c.w.U32(id);
      if (!WriteHandle(L, idx, c.w)) return Fail(c, "a userdata that isn't a game handle (Unit, Trigger, Group, ...) can't be saved");
      return true;
    }
    case LUA_TTHREAD: return Fail(c, "a coroutine can't be saved");
    default: return Fail(c, std::string("a ") + lua_typename(L, t) + " value can't be saved");
  }
}

// ---- restore ------------------------------------------------------------------------------------------------------
struct LoadCtx {
  lua_State* L;
  Reader& r;
  int objs, byPath, snaps;
  std::unordered_map<uint32_t, std::pair<uint32_t, int>> ups;   // upvalue -> the function (id) and index holding it
  std::unordered_map<uint32_t, uint32_t> chunks;                 // the save's chunk number -> this state's
  std::string err;
  int depth = 0;
};

bool Bad(LoadCtx& c, const std::string& what) {
  if (c.err.empty()) c.err = what;
  return false;
}

void Keep(LoadCtx& c, uint32_t id) {
  lua_pushvalue(c.L, -1);
  lua_rawseti(c.L, c.objs, id);
}

bool R(LoadCtx& c);

uint8_t PeekTag(Reader& r) {
  Reader copy = r;
  return copy.U8();
}

// Puts permanent table `t` back to its snapshot, then reads the changes the save made to it.
bool ReadDiff(LoadCtx& c, int t) {
  lua_State* L = c.L;
  lua_pushvalue(L, t);
  const bool hasSnap = lua_rawget(L, c.snaps) == LUA_TTABLE;
  const int snap = lua_gettop(L);
  lua_pushnil(L);
  while (lua_next(L, t) != 0) {
    lua_pop(L, 1);
    lua_pushvalue(L, -1);
    lua_pushnil(L);
    lua_rawset(L, t);   // clearing a field that exists is allowed while lua_next walks the table
  }
  if (hasSnap) {
    lua_pushnil(L);
    while (lua_next(L, snap) != 0) {
      lua_pushvalue(L, -2);
      lua_insert(L, -2);
      lua_rawset(L, t);
    }
  }
  lua_pop(L, 1);
  for (;;) {
    if (PeekTag(c.r) == kEnd) { c.r.U8(); break; }
    if (!R(c) || !R(c)) return false;
    if (lua_isnil(L, -2)) return Bad(c, "a table key is nil");
    lua_rawset(L, t);
  }
  for (;;) {
    if (PeekTag(c.r) == kEnd) { c.r.U8(); break; }
    if (!R(c)) return false;
    lua_pushnil(L);
    lua_rawset(L, t);
  }
  return c.r.Ok();
}

bool ReadMetatable(LoadCtx& c, int t, bool keepSame) {
  lua_State* L = c.L;
  if (!R(c)) return false;
  if (lua_isnil(L, -1)) {
    lua_pop(L, 1);
    if (!keepSame) return true;
    if (lua_getmetatable(L, t)) { lua_pop(L, 1); lua_pushnil(L); lua_setmetatable(L, t); }
    return true;
  }
  if (!lua_istable(L, -1)) return Bad(c, "a metatable isn't a table");
  if (keepSame && lua_getmetatable(L, t)) {
    const bool same = lua_rawequal(L, -1, -2) != 0;
    lua_pop(L, 1);
    if (same) { lua_pop(L, 1); return true; }
  }
  lua_setmetatable(L, t);
  return true;
}

bool ReadFunction(LoadCtx& c) {
  lua_State* L = c.L;
  const uint32_t id = c.r.U32(), chunk = c.r.U32(), index = c.r.U32();
  const int nups = c.r.U8();
  if (!c.r.Ok()) return Bad(c, "the save ends early");
  const auto at = c.chunks.find(chunk);
  if (at == c.chunks.end() || index >= gChunks[at->second].protos.size())
    return Bad(c, "a function names a prototype the script hasn't");
  const void* proto = gChunks[at->second].protos[index];
  if (op2lua_proto_upvalues(proto) != nups) return Bad(c, "a function's upvalues don't match the script");
  op2lua_push_closure(L, proto);
  const int f = lua_gettop(L);
  Keep(c, id);
  for (int i = 1; i <= nups; ++i) {
    const uint8_t tag = c.r.U8();
    const uint32_t uid = c.r.U32();
    if (tag == kUpNew) {
      c.ups[uid] = { id, i };
      if (!R(c)) return false;
      lua_setupvalue(L, f, i);
    } else if (tag == kUpRef) {
      const auto it = c.ups.find(uid);
      if (it == c.ups.end()) return Bad(c, "a shared upvalue comes before its function");
      lua_rawgeti(L, c.objs, it->second.first);
      lua_upvaluejoin(L, f, i, -1, it->second.second);
      lua_pop(L, 1);
    } else {
      return Bad(c, "a broken upvalue");
    }
  }
  return true;
}

bool R(LoadCtx& c) {
  lua_State* L = c.L;
  if (++c.depth > kMaxDepth + 10) return Bad(c, "values nested too deep");
  struct Leave { int& d; ~Leave() { --d; } } leave{ c.depth };
  if (!lua_checkstack(L, 10)) return Bad(c, "out of Lua stack");
  const uint8_t tag = c.r.U8();
  if (!c.r.Ok()) return Bad(c, "the save ends early");
  switch (tag) {
    case kNil: lua_pushnil(L); return true;
    case kFalse: lua_pushboolean(L, 0); return true;
    case kTrue: lua_pushboolean(L, 1); return true;
    case kInt: lua_pushinteger(L, c.r.I64()); return c.r.Ok();
    case kNum: lua_pushnumber(L, c.r.F64()); return c.r.Ok();
    case kStr: { const std::string s = c.r.Str(); if (!c.r.Ok()) return Bad(c, "the save ends early");
                 lua_pushlstring(L, s.data(), s.size()); return true; }
    case kRef: {
      const uint32_t id = c.r.U32();
      if (lua_rawgeti(L, c.objs, id) == LUA_TNIL) { lua_pop(L, 1); return Bad(c, "a reference to nothing"); }
      return true;
    }
    case kPerm:
    case kPermTable: {
      const uint32_t id = c.r.U32();
      const std::string path = c.r.Str();
      if (!c.r.Ok()) return Bad(c, "the save ends early");
      lua_pushlstring(L, path.data(), path.size());
      if (lua_rawget(L, c.byPath) == LUA_TNIL) {
        lua_pop(L, 1);
        return Bad(c, "the save names " + PathText(path) + ", which this OP2Lua hasn't");
      }
      Keep(c, id);
      if (tag == kPerm) return true;
      if (!lua_istable(L, -1)) return Bad(c, PathText(path) + " isn't a table here");
      const int t = lua_gettop(L);
      return ReadDiff(c, t) && ReadMetatable(c, t, true);
    }
    case kTable: {
      const uint32_t id = c.r.U32();
      lua_newtable(L);
      const int t = lua_gettop(L);
      Keep(c, id);
      for (;;) {
        if (PeekTag(c.r) == kEnd) { c.r.U8(); break; }
        if (!R(c) || !R(c)) return false;
        if (lua_isnil(L, -2) || (lua_type(L, -2) == LUA_TNUMBER && std::isnan(lua_tonumber(L, -2))))
          return Bad(c, "a table key is nil");
        lua_rawset(L, t);
      }
      return ReadMetatable(c, t, false);
    }
    case kFunc: return ReadFunction(c);
    case kHandle: {
      const uint32_t id = c.r.U32();
      const uint8_t kind = c.r.U8();
      if (!c.r.Ok() || !PushHandle(L, kind, c.r)) return Bad(c, "a broken game handle");
      Keep(c, id);
      return true;
    }
    default: return Bad(c, "an unknown value in the save");
  }
}

}  // namespace

// ---- chunks and permanents ----------------------------------------------------------------------------------------

void Reset() {
  gChunks.clear();
  gPlace.clear();
}

void AddChunk(lua_State* L, const std::string& name, std::string_view source) {
  const void* main = op2lua_function_proto(L, lua_gettop(L));
  if (main == nullptr) return;
  Chunk c;
  c.name = name;
  c.hash = Hash(source);
  c.protos.resize(op2lua_proto_list(main, nullptr, 0));
  op2lua_proto_list(main, c.protos.data(), static_cast<int>(c.protos.size()));
  const uint32_t at = static_cast<uint32_t>(gChunks.size());
  for (uint32_t i = 0; i < c.protos.size(); ++i) gPlace[c.protos[i]] = { at, i };
  gChunks.push_back(std::move(c));
  PushRegTable(L, &kChunkFns);   // keeps the chunk's prototypes alive (the places above point at them)
  lua_pushvalue(L, -2);
  lua_rawseti(L, -2, static_cast<lua_Integer>(at) + 1);
  lua_pop(L, 1);
}

void SnapshotPermanents(lua_State* L) {
  lua_newtable(L);
  const int byObj = lua_gettop(L);
  lua_newtable(L);
  const int byPath = lua_gettop(L);
  lua_newtable(L);
  const int snaps = lua_gettop(L);

  // Breadth first from each root in turn, keys in sorted order: every state with the same libraries gives every value
  // the same path. The globals go first, so a table they reach is tracked (its changes saved).
  struct Item { int ref; std::string path; };
  auto walk = [&](const char* rootName, bool tracked, bool skipIntegerKeys) {
    std::vector<Item> queue;
    queue.push_back({ luaL_ref(L, LUA_REGISTRYINDEX), rootName });
    for (size_t qi = 0; qi < queue.size(); ++qi) {
      lua_rawgeti(L, LUA_REGISTRYINDEX, queue[qi].ref);
      luaL_unref(L, LUA_REGISTRYINDEX, queue[qi].ref);
      const int v = lua_gettop(L);
      lua_pushvalue(L, v);
      const bool known = lua_rawget(L, byObj) != LUA_TNIL;
      lua_pop(L, 1);
      if (known) { lua_pop(L, 1); continue; }
      const std::string path = queue[qi].path;   // a copy: the queue grows below
      lua_pushvalue(L, v);
      lua_pushlstring(L, path.data(), path.size());
      lua_rawset(L, byObj);
      lua_pushlstring(L, path.data(), path.size());
      lua_pushvalue(L, v);
      lua_rawset(L, byPath);
      if (lua_type(L, v) != LUA_TTABLE) { lua_pop(L, 1); continue; }
      if (tracked) {
        lua_pushvalue(L, v);
        lua_newtable(L);
        lua_pushnil(L);
        while (lua_next(L, v) != 0) {
          lua_pushvalue(L, -2);
          lua_insert(L, -2);
          lua_rawset(L, -4);
        }
        lua_rawset(L, snaps);
      }
      std::vector<std::pair<std::string, int>> kids;   // segment, ref of the value
      const bool atRoot = qi == 0;
      lua_pushnil(L);
      while (lua_next(L, v) != 0) {
        std::string seg;
        const bool skip = atRoot && skipIntegerKeys && lua_type(L, -2) == LUA_TNUMBER;
        if (!skip && Collectable(lua_type(L, -1)) && KeySegment(L, -2, seg)) {
          lua_pushvalue(L, -1);
          kids.emplace_back(seg, luaL_ref(L, LUA_REGISTRYINDEX));
        }
        lua_pop(L, 1);
      }
      if (lua_getmetatable(L, v)) kids.emplace_back("m", luaL_ref(L, LUA_REGISTRYINDEX));
      std::sort(kids.begin(), kids.end(), [](const auto& a, const auto& b) { return a.first < b.first; });
      for (auto& k : kids) queue.push_back({ k.second, path + '\x1f' + k.first });
      lua_pop(L, 1);
    }
  };
  lua_pushglobaltable(L);
  walk("G", true, false);
  lua_pushstring(L, "");
  if (lua_getmetatable(L, -1)) { lua_remove(L, -2); walk("S", false, false); } else lua_pop(L, 1);
  lua_pushvalue(L, LUA_REGISTRYINDEX);
  walk("R", false, true);   // the registry's numbered entries are references, made in no fixed order
  // ipairs' iterator, which no table holds: a script may keep the one ipairs gave it.
  if (lua_getglobal(L, "ipairs") == LUA_TFUNCTION) {
    lua_newtable(L);
    if (lua_pcall(L, 1, 1, 0) == LUA_OK) walk("I", false, false); else lua_pop(L, 1);
  } else {
    lua_pop(L, 1);
  }

  lua_pushvalue(L, snaps);
  lua_rawsetp(L, LUA_REGISTRYINDEX, &kSnapshots);
  lua_pushvalue(L, byPath);
  lua_rawsetp(L, LUA_REGISTRYINDEX, &kPermByPath);
  lua_pushvalue(L, byObj);
  lua_rawsetp(L, LUA_REGISTRYINDEX, &kPermByObj);
  lua_pop(L, 3);
}

bool Save(lua_State* L, int idx, Writer& w, std::string& err) {
  idx = lua_absindex(L, idx);
  lua_newtable(L);
  const int seen = lua_gettop(L);
  PushRegTable(L, &kPermByObj);
  PushRegTable(L, &kSnapshots);
  Writer values;
  SaveCtx c{ L, values, seen, seen + 1, seen + 2 };
  c.chunksUsed.assign(gChunks.size(), false);
  bool ok = W(c, idx);
  // Then every tracked table the script changed and the root didn't reach as a change (the game table, units[...]).
  uint32_t extra = 0;
  Writer tail;
  if (ok) {
    std::vector<int> refs;
    lua_pushnil(L);
    while (lua_next(L, c.snaps) != 0) {
      lua_pop(L, 1);
      lua_pushvalue(L, -1);
      refs.push_back(luaL_ref(L, LUA_REGISTRYINDEX));
    }
    for (int ref : refs) {
      lua_rawgeti(L, LUA_REGISTRYINDEX, ref);
      luaL_unref(L, LUA_REGISTRYINDEX, ref);
      const int t = lua_gettop(L);
      lua_pushvalue(L, t);
      const bool written = lua_rawget(L, c.seen) != LUA_TNIL;
      lua_pop(L, 1);
      bool changed = false;
      if (ok && !written) {
        lua_pushvalue(L, t);
        lua_rawget(L, c.snaps);
        const int s = lua_gettop(L);
        lua_pushnil(L);
        while (!changed && lua_next(L, t) != 0) {
          lua_pushvalue(L, -2);
          lua_rawget(L, s);
          changed = lua_rawequal(L, -1, -2) == 0;
          lua_pop(L, changed ? 3 : 2);
        }
        lua_pushnil(L);
        while (!changed && lua_next(L, s) != 0) {
          lua_pushvalue(L, -2);
          changed = lua_rawget(L, t) == LUA_TNIL;
          lua_pop(L, changed ? 3 : 2);
        }
        lua_pop(L, 1);
      }
      if (changed) {
        ok = W(c, t);
        ++extra;
      }
      lua_pop(L, 1);
    }
  }
  lua_settop(L, seen - 1);
  if (!ok) { err = c.err; return false; }
  // The chunks a function in the save came from (the others needn't match on load), then the values.
  uint32_t used = 0;
  for (bool u : c.chunksUsed) used += u ? 1 : 0;
  w.U32(used);
  for (size_t i = 0; i < gChunks.size(); ++i) {
    if (!c.chunksUsed[i]) continue;
    w.U32(static_cast<uint32_t>(i));
    w.Str(gChunks[i].name);
    w.I64(static_cast<int64_t>(gChunks[i].hash));
    w.U32(static_cast<uint32_t>(gChunks[i].protos.size()));
  }
  w.U32(extra);
  w.Bytes(values.out.data(), values.out.size());
  return true;
}

bool Restore(lua_State* L, Reader& r, std::string& err) {
  const uint32_t used = r.U32();
  if (!r.Ok() || used > 64) { err = "the save's list of scripts is broken"; return false; }
  std::unordered_map<uint32_t, uint32_t> chunks;
  for (uint32_t i = 0; i < used; ++i) {
    const uint32_t savedAt = r.U32();
    const std::string name = r.Str();
    const uint64_t hash = static_cast<uint64_t>(r.I64());
    const uint32_t protos = r.U32();
    if (!r.Ok()) { err = "the save's list of scripts is broken"; return false; }
    const auto it = std::find_if(gChunks.begin(), gChunks.end(), [&](const Chunk& k) { return k.name == name; });
    if (it == gChunks.end()) { err = "the save needs the mission's " + name + ", which isn't loaded"; return false; }
    if (it->hash != hash || it->protos.size() != protos) {
      err = "the save was made with another version of the " + name;
      return false;
    }
    chunks[savedAt] = static_cast<uint32_t>(it - gChunks.begin());
  }
  const uint32_t extra = r.U32();
  if (!r.Ok()) { err = "the save ends early"; return false; }
  const int base = lua_gettop(L);
  lua_newtable(L);
  const int objs = lua_gettop(L);
  PushRegTable(L, &kPermByPath);
  PushRegTable(L, &kSnapshots);
  LoadCtx c{ L, r, objs, objs + 1, objs + 2 };
  c.chunks = std::move(chunks);
  bool ok = R(c);
  const int root = lua_gettop(L);
  for (uint32_t i = 0; ok && i < extra; ++i) {
    ok = R(c);
    if (ok) lua_settop(L, root);
  }
  if (!ok) {
    lua_settop(L, base);
    err = c.err.empty() ? "the save is broken" : c.err;
    return false;
  }
  lua_replace(L, objs);   // the root, in the first slot; the helpers go
  lua_settop(L, objs);
  return true;
}

}  // namespace OP2Lua::Persist
