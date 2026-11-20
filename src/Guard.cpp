// Guard.cpp - see Guard.h.

#include "Guard.h"

#include <cstdio>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <eh.h>

namespace OP2Lua::Guard {

namespace {

thread_local int tDepth = 0;
thread_local _se_translator_function tPrevious = nullptr;

const char* FaultName(DWORD code) {
  switch (code) {
    case EXCEPTION_ACCESS_VIOLATION:      return "access violation";
    case EXCEPTION_IN_PAGE_ERROR:         return "in-page error";
    case EXCEPTION_ARRAY_BOUNDS_EXCEEDED: return "array bounds exceeded";
    case EXCEPTION_INT_DIVIDE_BY_ZERO:    return "integer divide by zero";
    case EXCEPTION_INT_OVERFLOW:          return "integer overflow";
    case EXCEPTION_FLT_DIVIDE_BY_ZERO:    return "floating-point divide by zero";
    case EXCEPTION_FLT_INVALID_OPERATION: return "floating-point invalid operation";
    case EXCEPTION_ILLEGAL_INSTRUCTION:   return "illegal instruction";
    case EXCEPTION_PRIV_INSTRUCTION:      return "privileged instruction";
    case EXCEPTION_STACK_OVERFLOW:        return "stack overflow";
    default:                              return nullptr;
  }
}

// The C runtime calls this on the faulting thread when a fault reaches a C++ catch (/EHa); what it throws is what
// the catch sees.
void Translate(unsigned code, EXCEPTION_POINTERS* info) {
  throw GameFault(code, info->ExceptionRecord->ExceptionAddress, Describe(info));
}

}  // namespace

Scope::Scope() {
  if (tDepth++ == 0) tPrevious = _set_se_translator(&Translate);
}

Scope::~Scope() {
  if (--tDepth == 0) _set_se_translator(tPrevious);
}

bool Active() { return tDepth > 0; }

std::string Describe(const _EXCEPTION_POINTERS* info) {
  const EXCEPTION_RECORD& r = *info->ExceptionRecord;
  char buf[160];
  const char* name = FaultName(r.ExceptionCode);
  if (name != nullptr) {
    std::snprintf(buf, sizeof buf, "game call faulted: %s at 0x%08X", name,
                  static_cast<unsigned>(reinterpret_cast<ULONG_PTR>(r.ExceptionAddress)));
  } else {
    std::snprintf(buf, sizeof buf, "game call faulted: exception 0x%08X at 0x%08X", static_cast<unsigned>(r.ExceptionCode),
                  static_cast<unsigned>(reinterpret_cast<ULONG_PTR>(r.ExceptionAddress)));
  }
  std::string out = buf;
  // An access violation says what it touched (ExceptionInformation: 0 read, 1 write, 8 execute; then the address).
  if ((r.ExceptionCode == EXCEPTION_ACCESS_VIOLATION || r.ExceptionCode == EXCEPTION_IN_PAGE_ERROR) &&
      r.NumberParameters >= 2) {
    const ULONG_PTR rw = r.ExceptionInformation[0];
    std::snprintf(buf, sizeof buf, " %s 0x%08X", rw == 0 ? "reading" : rw == 1 ? "writing" : rw == 8 ? "executing" : "at",
                  static_cast<unsigned>(r.ExceptionInformation[1]));
    out += buf;
  }
  return out;
}

}  // namespace OP2Lua::Guard
