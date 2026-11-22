#pragma once

#include <string>

// sol2 - header-only C++ <-> Lua bindings. See third_party/README.md for how to add it.
#include <sol/sol.hpp>

namespace OP2Lua {

// Owns the embedded Lua interpreter and the bindings to the game.
//
// There is a single shared state for the running mission. (OP2 runs one mission at a time.) It opens only the
// libraries a mission may use (see Init) and counts the instructions each call runs (Budget).
class LuaHost {
 public:
  static LuaHost& Get();

  // Create the Lua state, open libraries, install the game bindings. Idempotent.
  void Init();
  // Destroy the Lua state.
  void Shutdown();

  sol::state& lua() { return lua_; }

  // Run a Lua script file in the main state.
  // Returns true on success. On a Lua error it logs the traceback and shows a message box,
  // and returns false - it never lets the error escape into the game's C/C++ code.
  bool RunFile(const std::string& path);

 private:
  sol::state lua_;
  bool initialized_ = false;
};

// The limit for runaway Lua: each call from the game into the mission's Lua (on_init, on_tick, an event, a trigger's
// function, the script's first run, its placement, and each after/every/when function the scheduler runs) may run
// kInstructions Lua instructions. Past that the call fails with a Lua error naming where it was running, and stays
// failed until it returns (a pcall inside the script can't catch it and carry on); the error is logged and the call
// dropped, and the game goes on. Instructions are counted, not time, so every machine in a multiplayer game stops a
// call at the same point. Time spent inside a game call isn't counted.
namespace Budget {
constexpr long long kInstructions = 25000000;
constexpr int kHookEvery = 1000;   // the count hook's interval: where a call that runs out stops
// Held around a call into the mission's Lua. The outermost one starts a fresh budget; one made inside a running call
// (an event the call set off) shares it, so a loop can't renew its own budget.
class Call {
 public:
  Call();
  ~Call();
  Call(const Call&) = delete;
  Call& operator=(const Call&) = delete;
  // Whether this call ran out of instructions.
  bool Exhausted() const;
};
}  // namespace Budget

// Installs the author-facing API (game.*, players, units, scheduler, ...). Defined in Bindings.cpp.
void RegisterBindings(sol::state& lua);

// The current game mark (100 ticks), for the logger's per-line stamp. Defined in Bindings.cpp (which
// can include Game.h cleanly); installed via Log's SetMarkProvider during core init.
int CurrentMarkForLog();

}  // namespace OP2Lua
