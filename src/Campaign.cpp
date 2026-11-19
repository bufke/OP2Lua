// campaign.* - a campaign mission's own data, saved when it ends and read by the next mission: the base the player
// built, colony values, whatever the next mission rebuilds its start from.
//
//   campaign.save(name, tbl)  -> true, or false and why
//   campaign.load(name)       -> the table, or nil and why
//   campaign.delete(name)     -> true when a file was removed
//
// The file is <mission folder>\<name>.carry, JSON. It holds data only: numbers, strings, booleans and tables of
// them. It is read by the parser below, never run as Lua. Integer and float numbers stay what they were; a table's
// integer keys come back as integers and its string keys as strings (save refuses a string key that reads as an
// integer, which would come back as one). A file is at most kMaxBytes and 64 tables deep.
//
// Nothing here touches the game, so the calls are safe from any callback. Each machine reads its own folder: in a
// multiplayer game the players' files can differ, so a multiplayer mission must not build anything from one.

#include "Campaign.h"

#include <windows.h>

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cctype>
#include <cstring>
#include <fstream>
#include <iterator>
#include <stdexcept>
#include <string>
#include <tuple>
#include <vector>

#include "Log.h"
#include "Mission.h"

namespace OP2Lua::Campaign {
namespace {

constexpr size_t kMaxBytes = 8u << 20;
constexpr int    kMaxDepth = 64;
constexpr char   kFormat[] = "op2lua-carry";

// A name is 1-64 letters, digits, '_' or '-', and not a Windows device name (CON, NUL, COM1 ...), which would open
// the device whatever the extension.
bool ValidName(const std::string& n, std::string& why) {
  if (n.empty() || n.size() > 64) { why = "a name is 1 to 64 characters"; return false; }
  for (unsigned char c : n) {
    if (!(std::isalnum(c) || c == '_' || c == '-')) {
      why = "a name is letters, digits, '_' and '-' only (no paths, dots or spaces)"; return false;
    }
  }
  std::string u = n;
  std::transform(u.begin(), u.end(), u.begin(), [](unsigned char c) { return char(std::toupper(c)); });
  static const char* const kDevices[] = { "CON", "PRN", "AUX", "NUL" };
  bool device = std::find_if(std::begin(kDevices), std::end(kDevices), [&](const char* d) { return u == d; })
                != std::end(kDevices);
  if (u.size() == 4 && (u.compare(0, 3, "COM") == 0 || u.compare(0, 3, "LPT") == 0) && u[3] >= '1' && u[3] <= '9')
    device = true;
  if (device) { why = "'" + n + "' is a Windows device name"; return false; }
  return true;
}

std::string PathFor(const std::string& name) { return ::OP2Lua::Mission::Dir() + "\\" + name + ".carry"; }

// A string that Lua would read back as this integer key: an optional '-', then digits with no leading zero.
bool CanonicalInteger(const char* s, size_t len, long long& out) {
  if (len == 0 || len > 20) return false;
  size_t i = (s[0] == '-') ? 1 : 0;
  if (i == len || (s[i] == '0' && len - i > 1) || (s[0] == '-' && s[1] == '0')) return false;
  for (size_t j = i; j < len; ++j) if (s[j] < '0' || s[j] > '9') return false;
  errno = 0;
  char* end = nullptr;
  const std::string str(s, len);
  out = std::strtoll(str.c_str(), &end, 10);
  return errno == 0 && end == str.c_str() + len;
}

// ---- writing ----

struct Writer {
  lua_State* L;
  std::string out;

  [[noreturn]] void Fail(const std::string& path, const std::string& what) {
    std::string where = path.empty() ? std::string("the table") : path;
    if (where.size() > 80) where = "..." + where.substr(where.size() - 77);
    throw std::runtime_error(where + ": " + what);
  }
  void Check(const std::string& path) {
    if (out.size() > kMaxBytes) Fail(path, "the data is over " + std::to_string(kMaxBytes >> 20) + " MB");
  }

  void String(const char* s, size_t len) {
    out += '"';
    for (size_t i = 0; i < len; ++i) {
      const unsigned char c = static_cast<unsigned char>(s[i]);
      switch (c) {
        case '"':  out += "\\\""; break;
        case '\\': out += "\\\\"; break;
        case '\n': out += "\\n";  break;
        case '\r': out += "\\r";  break;
        case '\t': out += "\\t";  break;
        default:
          if (c < 0x20) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); out += b; }
          else out += static_cast<char>(c);   // other bytes as they are; the reader takes them back unchanged
      }
    }
    out += '"';
  }

  void Value(int idx, int depth, const std::string& path) {
    switch (lua_type(L, idx)) {
      case LUA_TBOOLEAN: out += lua_toboolean(L, idx) ? "true" : "false"; break;
      case LUA_TNUMBER:
        if (lua_isinteger(L, idx)) {
          out += std::to_string(static_cast<long long>(lua_tointeger(L, idx)));
        } else {
          const double d = lua_tonumber(L, idx);
          if (!std::isfinite(d)) Fail(path, "a number that is inf or nan can't be saved");
          char b[40];
          std::snprintf(b, sizeof b, "%.17g", d);
          out += b;
          if (!std::strpbrk(b, ".eE")) out += ".0";   // stays a float when read back
        }
        break;
      case LUA_TSTRING: { size_t len = 0; const char* s = lua_tolstring(L, idx, &len); String(s, len); break; }
      case LUA_TTABLE:  Table(idx, depth + 1, path); break;
      default: Fail(path, std::string("a ") + lua_typename(L, lua_type(L, idx)) + " can't be saved (numbers, "
                          "strings, booleans and tables only)");
    }
    Check(path);
  }

  struct Key { bool isInt; long long i; std::string s; };

  void Table(int idx, int depth, const std::string& path) {
    if (depth > kMaxDepth) Fail(path, "tables nested over " + std::to_string(kMaxDepth) + " deep (or a table that "
                                      "holds itself)");
    if (!lua_checkstack(L, 4)) Fail(path, "out of Lua stack");
    idx = lua_absindex(L, idx);
    std::vector<Key> keys;
    long long maxInt = 0;
    bool allInt = true;
    lua_pushnil(L);
    while (lua_next(L, idx) != 0) {
      lua_pop(L, 1);   // the value; the key stays for lua_next
      if (lua_type(L, -1) == LUA_TNUMBER && lua_isinteger(L, -1)) {
        const long long k = lua_tointeger(L, -1);
        keys.push_back({ true, k, {} });
        if (k < 1) allInt = false; else maxInt = std::max(maxInt, k);
      } else if (lua_type(L, -1) == LUA_TSTRING) {
        size_t len = 0; const char* s = lua_tolstring(L, -1, &len);
        long long dummy;
        if (CanonicalInteger(s, len, dummy)) {
          lua_pop(L, 1);
          Fail(path, "the string key \"" + std::string(s, len) + "\" would come back as the number " +
                     std::string(s, len) + "; use the number or another name");
        }
        keys.push_back({ false, 0, std::string(s, len) });
        allInt = false;
      } else {
        const std::string t = lua_typename(L, lua_type(L, -1));
        lua_pop(L, 1);
        Fail(path, "a " + t + " key can't be saved (keys are strings or whole numbers)");
      }
    }
    if (allInt && static_cast<size_t>(maxInt) == keys.size() && !keys.empty()) {   // a list 1..n
      out += '[';
      for (long long i = 1; i <= maxInt; ++i) {
        if (i > 1) out += ',';
        lua_rawgeti(L, idx, i);
        Value(-1, depth, path + "[" + std::to_string(i) + "]");
        lua_pop(L, 1);
      }
      out += ']';
      return;
    }
    std::sort(keys.begin(), keys.end(), [](const Key& a, const Key& b) {
      if (a.isInt != b.isInt) return a.isInt;
      return a.isInt ? a.i < b.i : a.s < b.s;
    });
    out += '{';
    bool first = true;
    for (const Key& k : keys) {
      if (!first) out += ',';
      first = false;
      std::string sub;
      if (k.isInt) {
        const std::string s = std::to_string(k.i);
        String(s.data(), s.size());
        lua_pushinteger(L, k.i);
        sub = path + "[" + s + "]";
      } else {
        String(k.s.data(), k.s.size());
        lua_pushlstring(L, k.s.data(), k.s.size());
        sub = path.empty() ? k.s : path + "." + k.s;
      }
      out += ':';
      lua_rawget(L, idx);
      Value(-1, depth, sub);
      lua_pop(L, 1);
    }
    out += '}';
  }
};

// ---- reading ----

struct Reader {
  lua_State* L;
  const char* p;
  const char* end;
  const char* begin;

  [[noreturn]] void Fail(const std::string& what) {
    throw std::runtime_error(what + " at byte " + std::to_string(p - begin));
  }
  void Space() { while (p < end && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r')) ++p; }
  void Expect(char c) { Space(); if (p >= end || *p != c) Fail(std::string("expected '") + c + "'"); ++p; }
  bool Word(const char* w) {
    const size_t n = std::strlen(w);
    if (static_cast<size_t>(end - p) >= n && std::memcmp(p, w, n) == 0) { p += n; return true; }
    return false;
  }

  static void Utf8(std::string& s, unsigned cp) {
    if (cp < 0x80) s += char(cp);
    else if (cp < 0x800) { s += char(0xC0 | (cp >> 6)); s += char(0x80 | (cp & 0x3F)); }
    else if (cp < 0x10000) { s += char(0xE0 | (cp >> 12)); s += char(0x80 | ((cp >> 6) & 0x3F)); s += char(0x80 | (cp & 0x3F)); }
    else { s += char(0xF0 | (cp >> 18)); s += char(0x80 | ((cp >> 12) & 0x3F)); s += char(0x80 | ((cp >> 6) & 0x3F)); s += char(0x80 | (cp & 0x3F)); }
  }
  unsigned Hex4() {
    if (end - p < 4) Fail("a short \\u escape");
    unsigned v = 0;
    for (int i = 0; i < 4; ++i, ++p) {
      const char c = *p;
      v <<= 4;
      if (c >= '0' && c <= '9') v |= c - '0';
      else if (c >= 'a' && c <= 'f') v |= c - 'a' + 10;
      else if (c >= 'A' && c <= 'F') v |= c - 'A' + 10;
      else Fail("a bad \\u escape");
    }
    return v;
  }
  std::string Str() {
    Expect('"');
    std::string s;
    while (true) {
      if (p >= end) Fail("an unfinished string");
      const char c = *p++;
      if (c == '"') return s;
      if (c != '\\') { s += c; continue; }
      if (p >= end) Fail("an unfinished string");
      switch (*p++) {
        case '"': s += '"'; break;   case '\\': s += '\\'; break;  case '/': s += '/'; break;
        case 'b': s += '\b'; break;  case 'f': s += '\f'; break;   case 'n': s += '\n'; break;
        case 'r': s += '\r'; break;  case 't': s += '\t'; break;
        case 'u': {
          unsigned cp = Hex4();
          if (cp >= 0xD800 && cp < 0xDC00 && end - p >= 6 && p[0] == '\\' && p[1] == 'u') {
            p += 2;
            const unsigned lo = Hex4();
            if (lo >= 0xDC00 && lo < 0xE000) cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            else Fail("a bad surrogate pair");
          }
          Utf8(s, cp);
          break;
        }
        default: Fail("a bad escape");
      }
    }
  }

  void Number() {
    const char* start = p;
    bool isFloat = false;
    if (p < end && *p == '-') ++p;
    while (p < end && ((*p >= '0' && *p <= '9') || *p == '.' || *p == 'e' || *p == 'E' || *p == '+' || *p == '-')) {
      if (*p == '.' || *p == 'e' || *p == 'E') isFloat = true;
      ++p;
    }
    const std::string s(start, p);
    if (s.empty() || s == "-") Fail("a bad value");
    char* stop = nullptr;
    errno = 0;
    if (!isFloat) {
      const long long v = std::strtoll(s.c_str(), &stop, 10);
      if (errno == 0 && stop == s.c_str() + s.size()) { lua_pushinteger(L, static_cast<lua_Integer>(v)); return; }
    }
    errno = 0;
    const double d = std::strtod(s.c_str(), &stop);
    if (stop != s.c_str() + s.size() || !std::isfinite(d)) Fail("a bad number");
    lua_pushnumber(L, d);
  }

  // Pushes the next value.
  void Value(int depth) {
    if (depth > kMaxDepth) Fail("tables nested over " + std::to_string(kMaxDepth) + " deep");
    if (!lua_checkstack(L, 4)) Fail("out of Lua stack");
    Space();
    if (p >= end) Fail("the data ends early");
    switch (*p) {
      case '{': {
        ++p;
        lua_newtable(L);
        Space();
        if (p < end && *p == '}') { ++p; return; }
        while (true) {
          const std::string k = Str();
          Expect(':');
          long long ik;
          if (CanonicalInteger(k.data(), k.size(), ik)) lua_pushinteger(L, static_cast<lua_Integer>(ik));
          else lua_pushlstring(L, k.data(), k.size());
          Value(depth + 1);
          lua_rawset(L, -3);
          Space();
          if (p < end && *p == ',') { ++p; continue; }
          Expect('}');
          return;
        }
      }
      case '[': {
        ++p;
        lua_newtable(L);
        Space();
        if (p < end && *p == ']') { ++p; return; }
        for (lua_Integer i = 1;; ++i) {
          Value(depth + 1);
          lua_rawseti(L, -2, i);
          Space();
          if (p < end && *p == ',') { ++p; continue; }
          Expect(']');
          return;
        }
      }
      case '"': { const std::string s = Str(); lua_pushlstring(L, s.data(), s.size()); return; }
      default:
        if (Word("true"))  { lua_pushboolean(L, 1); return; }
        if (Word("false")) { lua_pushboolean(L, 0); return; }
        Number();
    }
  }
};

bool ReadFile(const std::string& path, std::string& data, std::string& why) {
  std::ifstream in(path, std::ios::binary | std::ios::ate);
  if (!in) { why = "no file"; return false; }
  const std::streamoff size = in.tellg();
  if (size < 0 || static_cast<unsigned long long>(size) > kMaxBytes) {
    why = "the file is over " + std::to_string(kMaxBytes >> 20) + " MB"; return false;
  }
  in.seekg(0);
  data.resize(static_cast<size_t>(size));
  if (size > 0 && !in.read(&data[0], size)) { why = "the file can't be read"; return false; }
  return true;
}

// Writes beside the old file, then swaps it in, so a failed write leaves the last save whole.
bool WriteFile(const std::string& path, const std::string& data, std::string& why) {
  const std::string tmp = path + ".tmp";
  {
    std::ofstream out(tmp, std::ios::binary | std::ios::trunc);
    if (!out || !out.write(data.data(), static_cast<std::streamsize>(data.size())) || !out.flush()) {
      why = "can't write " + tmp; out.close(); DeleteFileA(tmp.c_str()); return false;
    }
  }
  if (!MoveFileExA(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
    why = "can't replace " + path + " (error " + std::to_string(GetLastError()) + ")";
    DeleteFileA(tmp.c_str());
    return false;
  }
  return true;
}

std::tuple<bool, sol::object> Save(sol::object nameArg, sol::object data, sol::this_state ts) {
  lua_State* L = ts;
  sol::state_view lua(L);
  auto fail = [&](const std::string& name, const std::string& why) {
    LOG_MISSION(WARNING) << "campaign.save('" << name << "'): " << why;
    return std::make_tuple(false, sol::make_object(lua, why));
  };
  const std::string name = nameArg.is<std::string>() ? nameArg.as<std::string>() : std::string();
  std::string why;
  if (!nameArg.is<std::string>()) return fail(name, "the name must be a string");
  if (!ValidName(name, why)) return fail(name, why);
  if (data.get_type() != sol::type::table) return fail(name, "the data must be a table");

  const int top = lua_gettop(L);
  Writer w{ L, {} };
  try {
    w.out = std::string("{\"format\":\"") + kFormat + "\",\"version\":1,\"data\":";
    data.push(L);
    w.Table(-1, 0, "");
    w.out += "}\n";
    w.Check("");
  } catch (const std::exception& e) {
    lua_settop(L, top);
    return fail(name, e.what());
  }
  lua_settop(L, top);
  const std::string path = PathFor(name);
  if (!WriteFile(path, w.out, why)) return fail(name, why);
  LOG_MISSION(INFO) << "campaign.save('" << name << "'): " << w.out.size() << " bytes to " << path;
  return std::make_tuple(true, sol::make_object(lua, sol::lua_nil));
}

std::tuple<sol::object, sol::object> Load(sol::object nameArg, sol::this_state ts) {
  lua_State* L = ts;
  sol::state_view lua(L);
  const std::string name = nameArg.is<std::string>() ? nameArg.as<std::string>() : std::string();
  auto fail = [&](const std::string& why, bool warn) {
    if (warn) LOG_MISSION(WARNING) << "campaign.load('" << name << "'): " << why;
    else      LOG_MISSION(INFO)    << "campaign.load('" << name << "'): " << why;
    return std::make_tuple(sol::make_object(lua, sol::lua_nil), sol::make_object(lua, why));
  };
  std::string why;
  if (!nameArg.is<std::string>()) return fail("the name must be a string", true);
  if (!ValidName(name, why)) return fail(why, true);
  const std::string path = PathFor(name);
  std::string text;
  if (!ReadFile(path, text, why)) return fail(why == "no file" ? "no file " + path : why, why != "no file");

  const int top = lua_gettop(L);
  try {
    Reader r{ L, text.data(), text.data() + text.size(), text.data() };
    r.Value(0);
    r.Space();
    if (r.p != r.end) r.Fail("more after the data");
    if (!lua_istable(L, -1)) throw std::runtime_error("not a campaign file");
    lua_getfield(L, -1, "format");
    const char* f = lua_tostring(L, -1);
    if (!f || std::strcmp(f, kFormat) != 0) throw std::runtime_error("not a campaign file");
    lua_pop(L, 1);
    lua_getfield(L, -1, "version");
    if (!lua_isinteger(L, -1) || lua_tointeger(L, -1) != 1) throw std::runtime_error("a newer campaign file version");
    lua_pop(L, 1);
    lua_getfield(L, -1, "data");
    if (!lua_istable(L, -1)) throw std::runtime_error("the file holds no table");
    sol::object out(L, -1);
    lua_settop(L, top);
    LOG_MISSION(INFO) << "campaign.load('" << name << "'): " << text.size() << " bytes from " << path;
    return std::make_tuple(out, sol::make_object(lua, sol::lua_nil));
  } catch (const std::exception& e) {
    lua_settop(L, top);
    return fail(path + ": " + e.what(), true);
  }
}

bool Delete(sol::object nameArg) {
  const std::string name = nameArg.is<std::string>() ? nameArg.as<std::string>() : std::string();
  std::string why;
  if (!nameArg.is<std::string>() || !ValidName(name, why)) {
    LOG_MISSION(WARNING) << "campaign.delete('" << name << "'): " << (why.empty() ? "the name must be a string" : why);
    return false;
  }
  const std::string path = PathFor(name);
  const bool removed = DeleteFileA(path.c_str()) != 0;
  LOG_MISSION(INFO) << "campaign.delete('" << name << "'): " << (removed ? "removed " : "no file ") << path;
  return removed;
}

}  // namespace

void Register(sol::state& lua) {
  sol::table c = lua.create_named_table("campaign");
  c.set_function("save", &Save);
  c.set_function("load", &Load);
  c.set_function("delete", &Delete);
}

}  // namespace OP2Lua::Campaign
