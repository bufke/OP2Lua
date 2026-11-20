// Guard.h - game calls that fault become Lua errors instead of closing the game.
//
// While a Guard::Scope is alive on the thread (the core holds one while it runs mission code: callbacks, the
// scheduler, placement, queued orders), a hardware fault such as an access violation inside a game call becomes a
// GameFault C++ exception. The core is built with /EHa, so the exception unwinds the binding's C++ frames, and sol2's
// trampoline turns it into a Lua error: the error is logged with the callback it happened in, that callback stops,
// and the game carries on. The crash reporter (ErrorHandling.cpp) stays quiet for a fault inside a scope, which is
// caught; a fault anywhere else still closes the game with a full dump.
//
// A guard catches the fault, not what led to it: a game function stopped halfway can leave its state half-changed,
// so the bindings check their arguments before calling (a live unit, bay 1-6, a player in range).

#pragma once

#include <stdexcept>
#include <string>

struct _EXCEPTION_POINTERS;

namespace OP2Lua::Guard {

// A hardware fault inside guarded code: its exception code, where it happened, and a one-line description
// ("access violation at 0x0044ABCD reading 0x00000010").
class GameFault : public std::runtime_error {
 public:
  GameFault(unsigned code, const void* address, const std::string& what)
      : std::runtime_error(what), code_(code), address_(address) {}
  unsigned code() const { return code_; }
  const void* address() const { return address_; }

 private:
  unsigned code_;
  const void* address_;
};

// Faults become GameFault while one of these is alive on the thread. Scopes nest.
class Scope {
 public:
  Scope();
  ~Scope();
  Scope(const Scope&) = delete;
  Scope& operator=(const Scope&) = delete;
};

// Whether a Scope is alive on this thread (the crash reporter skips its dump then).
bool Active();

// The description GameFault carries, for a fault's exception record.
std::string Describe(const _EXCEPTION_POINTERS* info);

}  // namespace OP2Lua::Guard
