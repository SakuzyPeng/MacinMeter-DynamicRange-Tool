// This DLL exists only to satisfy the fixed plugin's shared.dll imports.
// None of those host-service entry points may be emulated by the core worker.

using Dword = unsigned long;
using Bool = int;
using Handle = void*;

struct ExceptionRecord;
struct Context;

static_assert(sizeof(Dword) == 4);
static_assert(sizeof(void*) == 8);

extern "C" __declspec(dllimport) void __stdcall RaiseFailFastException(
    ExceptionRecord* exception_record, Context* context_record,
    Dword flags) noexcept;
extern "C" __declspec(dllimport) Bool __stdcall
TerminateProcess(Handle process, unsigned int exit_code) noexcept;

namespace {

constexpr unsigned int kFailFastExitCode = 0xC0000409U;
constexpr unsigned int kShimMarker = 0x4D4D5348U;

}  // namespace

// All 13 plugin imports are aliases of this routine in fail_fast_shared.def.
// The x64 ABI permits one no-argument, non-returning implementation because it
// never observes the caller's arguments or produces a return value.
extern "C" [[noreturn]] void
macinmeter_shared_shim_fail_fast() noexcept {
  for (;;) {
    RaiseFailFastException(nullptr, nullptr, 0);
    TerminateProcess(reinterpret_cast<Handle>(-1LL), kFailFastExitCode);
  }
}

extern "C" unsigned int macinmeter_shared_shim_marker_v1() noexcept {
  return kShimMarker;
}
